use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::model::DeviceId;
use crate::tx::context::TxContext;
use crate::tx::strategy::TransactionStrategy;
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxPhase {
    Started,
    Preparing,
    Prepared,
    Committing,
    Verifying,
    Recovering,
    Committed,
    RollingBack,
    RolledBack,
    Failed,
    InDoubt,
}

impl TxPhase {
    pub fn requires_recovery(&self) -> bool {
        !matches!(
            self,
            Self::Committed | Self::RolledBack | Self::Failed
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxJournalRecord {
    pub tx_id: String,
    pub request_id: String,
    pub trace_id: String,
    pub phase: TxPhase,
    pub devices: Vec<DeviceId>,
    pub strategy: Option<TransactionStrategy>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

impl TxJournalRecord {
    pub fn started(context: &TxContext, devices: Vec<DeviceId>) -> Self {
        let now = now_unix_secs();
        Self {
            tx_id: context.tx_id.clone(),
            request_id: context.request_id.clone(),
            trace_id: context.trace_id.clone(),
            phase: TxPhase::Started,
            devices,
            strategy: None,
            error_code: None,
            error_message: None,
            created_at_unix_secs: now,
            updated_at_unix_secs: now,
        }
    }

    pub fn with_phase(mut self, phase: TxPhase) -> Self {
        self.phase = phase;
        self.updated_at_unix_secs = now_unix_secs();
        self
    }

    pub fn with_strategy(mut self, strategy: TransactionStrategy) -> Self {
        self.strategy = Some(strategy);
        self.updated_at_unix_secs = now_unix_secs();
        self
    }

    pub fn with_error(mut self, code: impl Into<String>, message: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self.error_message = Some(message.into());
        self.updated_at_unix_secs = now_unix_secs();
        self
    }
}

pub trait TxJournalStore: std::fmt::Debug + Send + Sync {
    fn put(&self, record: &TxJournalRecord) -> UnderlayResult<()>;

    fn get(&self, tx_id: &str) -> UnderlayResult<Option<TxJournalRecord>>;

    fn list_recoverable(&self) -> UnderlayResult<Vec<TxJournalRecord>>;
}

#[derive(Debug, Default)]
pub struct InMemoryTxJournalStore {
    records: Mutex<BTreeMap<String, TxJournalRecord>>,
}

impl TxJournalStore for InMemoryTxJournalStore {
    fn put(&self, record: &TxJournalRecord) -> UnderlayResult<()> {
        self.records
            .lock()
            .map_err(|_| UnderlayError::Internal("tx journal mutex poisoned".into()))?
            .insert(record.tx_id.clone(), record.clone());
        Ok(())
    }

    fn get(&self, tx_id: &str) -> UnderlayResult<Option<TxJournalRecord>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| UnderlayError::Internal("tx journal mutex poisoned".into()))?
            .get(tx_id)
            .cloned())
    }

    fn list_recoverable(&self) -> UnderlayResult<Vec<TxJournalRecord>> {
        let records = self
            .records
            .lock()
            .map_err(|_| UnderlayError::Internal("tx journal mutex poisoned".into()))?;
        Ok(records
            .values()
            .filter(|record| record.phase.requires_recovery())
            .cloned()
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileTxJournalStore {
    root: PathBuf,
}

impl JsonFileTxJournalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, tx_id: &str) -> PathBuf {
        self.root.join(format!("{}.json", journal_file_stem(tx_id)))
    }
}

impl TxJournalStore for JsonFileTxJournalStore {
    fn put(&self, record: &TxJournalRecord) -> UnderlayResult<()> {
        fs::create_dir_all(&self.root).map_err(journal_io_error)?;

        let path = self.path_for(&record.tx_id);
        let tmp_path = path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(record)
            .map_err(|err| UnderlayError::Internal(format!("serialize tx journal: {err}")))?;

        fs::write(&tmp_path, payload).map_err(journal_io_error)?;
        fs::rename(&tmp_path, &path).map_err(journal_io_error)?;
        Ok(())
    }

    fn get(&self, tx_id: &str) -> UnderlayResult<Option<TxJournalRecord>> {
        let path = self.path_for(tx_id);
        if !path.exists() {
            return Ok(None);
        }
        read_journal_record(&path).map(Some)
    }

    fn list_recoverable(&self) -> UnderlayResult<Vec<TxJournalRecord>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(journal_io_error)? {
            let path = entry.map_err(journal_io_error)?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let record = read_journal_record(&path)?;
            if record.phase.requires_recovery() {
                records.push(record);
            }
        }
        records.sort_by(|left, right| left.tx_id.cmp(&right.tx_id));
        Ok(records)
    }
}

fn read_journal_record(path: &Path) -> UnderlayResult<TxJournalRecord> {
    let payload = fs::read(path).map_err(journal_io_error)?;
    serde_json::from_slice(&payload)
        .map_err(|err| UnderlayError::Internal(format!("parse tx journal {:?}: {err}", path)))
}

fn journal_file_stem(tx_id: &str) -> String {
    tx_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn journal_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("tx journal io error: {err}"))
}
