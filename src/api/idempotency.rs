use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::api::request::ApplyOptions;
use crate::api::response::ApplyIntentResponse;
use crate::intent::UnderlayDomainIntent;
use crate::utils::atomic_file::atomic_write;
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

const MAX_IDEMPOTENCY_KEY_BYTES: usize = 125;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ApplyIdempotencyRecord {
    #[serde(default = "now_unix_secs")]
    stored_at_unix_secs: u64,
    fingerprint: String,
    response: ApplyIntentResponse,
}

impl ApplyIdempotencyRecord {
    pub(crate) fn new(fingerprint: String, response: ApplyIntentResponse) -> Self {
        Self {
            stored_at_unix_secs: now_unix_secs(),
            fingerprint,
            response,
        }
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn response(&self) -> &ApplyIntentResponse {
        &self.response
    }
}

pub(crate) trait ApplyIdempotencyStore: std::fmt::Debug + Send + Sync {
    fn get(&self, key: &str) -> UnderlayResult<Option<ApplyIdempotencyRecord>>;
    fn put(&self, key: &str, record: &ApplyIdempotencyRecord) -> UnderlayResult<()>;
}

#[derive(Debug, Default)]
pub(crate) struct InMemoryApplyIdempotencyStore {
    entries: Mutex<BTreeMap<String, ApplyIdempotencyRecord>>,
}

impl ApplyIdempotencyStore for InMemoryApplyIdempotencyStore {
    fn get(&self, key: &str) -> UnderlayResult<Option<ApplyIdempotencyRecord>> {
        let entries = self.entries.lock().map_err(|_| {
            UnderlayError::Internal("apply idempotency store lock poisoned".into())
        })?;
        Ok(entries.get(key).cloned())
    }

    fn put(&self, key: &str, record: &ApplyIdempotencyRecord) -> UnderlayResult<()> {
        let mut entries = self.entries.lock().map_err(|_| {
            UnderlayError::Internal("apply idempotency store lock poisoned".into())
        })?;
        entries.insert(key.to_string(), record.clone());
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JsonFileApplyIdempotencyStore {
    root: PathBuf,
}

impl JsonFileApplyIdempotencyStore {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(format!("{}.json", hex_encode(key)))
    }
}

impl ApplyIdempotencyStore for JsonFileApplyIdempotencyStore {
    fn get(&self, key: &str) -> UnderlayResult<Option<ApplyIdempotencyRecord>> {
        let path = self.path_for(key);
        if !path.exists() {
            return Ok(None);
        }
        read_record(&path).map(Some)
    }

    fn put(&self, key: &str, record: &ApplyIdempotencyRecord) -> UnderlayResult<()> {
        let path = self.path_for(key);
        let payload = serde_json::to_vec_pretty(record).map_err(|err| {
            UnderlayError::Internal(format!("serialize apply idempotency record: {err}"))
        })?;
        atomic_write(&path, &payload, idempotency_io_error)
    }
}

pub(crate) struct ApplyIdempotencyRegistry {
    entries: Mutex<BTreeMap<String, Arc<AsyncMutex<Option<ApplyIdempotencyRecord>>>>>,
    store: Arc<dyn ApplyIdempotencyStore>,
}

impl Default for ApplyIdempotencyRegistry {
    fn default() -> Self {
        Self::new(Arc::new(InMemoryApplyIdempotencyStore::default()))
    }
}

impl std::fmt::Debug for ApplyIdempotencyRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplyIdempotencyRegistry")
            .finish_non_exhaustive()
    }
}

impl ApplyIdempotencyRegistry {
    pub(crate) fn new(store: Arc<dyn ApplyIdempotencyStore>) -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            store,
        }
    }

    pub(crate) fn slot(
        &self,
        key: &str,
    ) -> UnderlayResult<Arc<AsyncMutex<Option<ApplyIdempotencyRecord>>>> {
        {
            let entries = self.entries.lock().map_err(|_| {
                UnderlayError::Internal("apply idempotency registry lock poisoned".into())
            })?;
            if let Some(slot) = entries.get(key) {
                return Ok(slot.clone());
            }
        }

        let stored_record = self.store.get(key)?;
        let mut entries = self.entries.lock().map_err(|_| {
            UnderlayError::Internal("apply idempotency registry lock poisoned".into())
        })?;
        Ok(entries
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(stored_record)))
            .clone())
    }

    pub(crate) fn put(
        &self,
        key: &str,
        record: &ApplyIdempotencyRecord,
    ) -> UnderlayResult<()> {
        self.store.put(key, record)
    }
}

#[derive(Serialize)]
struct ApplyDomainIdempotencyPayload<'a> {
    api: &'static str,
    intent: &'a UnderlayDomainIntent,
    options: &'a ApplyOptions,
}

pub(crate) fn apply_domain_fingerprint(
    intent: &UnderlayDomainIntent,
    options: &ApplyOptions,
) -> UnderlayResult<String> {
    serde_json::to_string(&ApplyDomainIdempotencyPayload {
        api: "apply_domain_intent",
        intent,
        options,
    })
    .map_err(|err| {
        UnderlayError::Internal(format!(
            "serialize apply domain idempotency fingerprint: {err}"
        ))
    })
}

pub(crate) fn normalize_idempotency_key(key: &str) -> UnderlayResult<String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "idempotency_key must not be empty".into(),
        ));
    }
    let key_len = key.len();
    if key_len > MAX_IDEMPOTENCY_KEY_BYTES {
        return Err(UnderlayError::InvalidIntent(format!(
            "idempotency_key must be at most {MAX_IDEMPOTENCY_KEY_BYTES} bytes after trimming; got {key_len} bytes"
        )));
    }
    Ok(key.to_string())
}

pub(crate) fn idempotency_payload_mismatch_error(key: &str) -> UnderlayError {
    UnderlayError::InvalidIntent(format!(
        "idempotency_key {key:?} was already used with a different apply payload"
    ))
}

fn read_record(path: &Path) -> UnderlayResult<ApplyIdempotencyRecord> {
    let payload = fs::read(path).map_err(idempotency_io_error)?;
    serde_json::from_slice(&payload).map_err(|err| {
        UnderlayError::Internal(format!("parse apply idempotency record {:?}: {err}", path))
    })
}

fn hex_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for &byte in value.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn idempotency_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("apply idempotency io error: {err}"))
}
