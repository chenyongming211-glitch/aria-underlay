use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;

use crate::api::request::ApplyOptions;
use crate::api::response::ApplyIntentResponse;
use crate::intent::UnderlayDomainIntent;
use crate::{UnderlayError, UnderlayResult};

#[derive(Clone)]
pub(crate) struct ApplyIdempotencyRecord {
    fingerprint: String,
    response: ApplyIntentResponse,
}

impl ApplyIdempotencyRecord {
    pub(crate) fn new(fingerprint: String, response: ApplyIntentResponse) -> Self {
        Self {
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

#[derive(Default)]
pub(crate) struct ApplyIdempotencyRegistry {
    entries: Mutex<BTreeMap<String, Arc<AsyncMutex<Option<ApplyIdempotencyRecord>>>>>,
}

impl std::fmt::Debug for ApplyIdempotencyRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplyIdempotencyRegistry")
            .finish_non_exhaustive()
    }
}

impl ApplyIdempotencyRegistry {
    pub(crate) fn slot(
        &self,
        key: &str,
    ) -> UnderlayResult<Arc<AsyncMutex<Option<ApplyIdempotencyRecord>>>> {
        let mut entries = self.entries.lock().map_err(|_| {
            UnderlayError::Internal("apply idempotency registry lock poisoned".into())
        })?;
        Ok(entries
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(None)))
            .clone())
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
    Ok(key.to_string())
}

pub(crate) fn idempotency_payload_mismatch_error(key: &str) -> UnderlayError {
    UnderlayError::InvalidIntent(format!(
        "idempotency_key {key:?} was already used with a different apply payload"
    ))
}
