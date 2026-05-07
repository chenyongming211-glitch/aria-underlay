use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;

use crate::engine::diff::ChangeSet;
use crate::model::DeviceId;
use crate::planner::device_plan::DeviceDesiredState;
use crate::tx::context::TxContext;
use crate::tx::strategy::TransactionStrategy;
use crate::utils::atomic_file::atomic_write;
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxPhase {
    Started,
    Preparing,
    Prepared,
    Committing,
    Verifying,
    FinalConfirming,
    Recovering,
    Committed,
    RollingBack,
    RolledBack,
    Failed,
    InDoubt,
    ForceResolved,
}

impl TxPhase {
    pub fn requires_recovery(&self) -> bool {
        !matches!(
            self,
            Self::Committed | Self::RolledBack | Self::Failed | Self::ForceResolved
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxJournalErrorEvent {
    pub phase: TxPhase,
    pub code: String,
    pub message: String,
    pub created_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxManualResolution {
    pub operator: String,
    pub reason: String,
    pub request_id: String,
    pub trace_id: String,
    pub resolved_at_unix_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxJournalRecord {
    pub tx_id: String,
    pub request_id: String,
    pub trace_id: String,
    pub phase: TxPhase,
    pub devices: Vec<DeviceId>,
    #[serde(default)]
    pub desired_states: Vec<DeviceDesiredState>,
    #[serde(default)]
    pub change_sets: Vec<ChangeSet>,
    pub strategy: Option<TransactionStrategy>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[serde(default)]
    pub error_history: Vec<TxJournalErrorEvent>,
    #[serde(default)]
    pub manual_resolution: Option<TxManualResolution>,
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
            desired_states: Vec::new(),
            change_sets: Vec::new(),
            strategy: None,
            error_code: None,
            error_message: None,
            error_history: Vec::new(),
            manual_resolution: None,
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

    pub fn with_desired_states(mut self, desired_states: Vec<DeviceDesiredState>) -> Self {
        self.desired_states = desired_states;
        self.updated_at_unix_secs = now_unix_secs();
        self
    }

    pub fn with_change_sets(mut self, change_sets: Vec<ChangeSet>) -> Self {
        self.change_sets = change_sets;
        self.updated_at_unix_secs = now_unix_secs();
        self
    }

    pub fn with_error(mut self, code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        let message = message.into();
        let now = now_unix_secs();
        self.error_code = Some(code.clone());
        self.error_message = Some(message.clone());
        self.error_history.push(TxJournalErrorEvent {
            phase: self.phase.clone(),
            code,
            message,
            created_at_unix_secs: now,
        });
        self.updated_at_unix_secs = now;
        self
    }

    pub fn with_manual_resolution(
        mut self,
        operator: impl Into<String>,
        reason: impl Into<String>,
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        let now = now_unix_secs();
        self.manual_resolution = Some(TxManualResolution {
            operator: operator.into(),
            reason: reason.into(),
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            resolved_at_unix_secs: now,
        });
        self.updated_at_unix_secs = now;
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
    locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl JsonFileTxJournalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            locks: Arc::new(DashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, tx_id: &str) -> UnderlayResult<PathBuf> {
        validate_journal_tx_id(tx_id)?;
        Ok(self.root.join(format!("{tx_id}.json")))
    }

    fn lock_for(&self, tx_id: &str) -> Arc<Mutex<()>> {
        self.locks
            .entry(tx_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .value()
            .clone()
    }
}

impl TxJournalStore for JsonFileTxJournalStore {
    fn put(&self, record: &TxJournalRecord) -> UnderlayResult<()> {
        let lock = self.lock_for(&record.tx_id);
        let _guard = lock
            .lock()
            .map_err(|_| UnderlayError::Internal("tx journal mutex poisoned".into()))?;

        let path = self.path_for(&record.tx_id)?;
        let payload = serde_json::to_vec_pretty(record)
            .map_err(|err| UnderlayError::Internal(format!("serialize tx journal: {err}")))?;

        atomic_write(&path, &payload, journal_io_error)?;
        Ok(())
    }

    fn get(&self, tx_id: &str) -> UnderlayResult<Option<TxJournalRecord>> {
        let path = self.path_for(tx_id)?;
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

fn validate_journal_tx_id(tx_id: &str) -> UnderlayResult<()> {
    if tx_id.is_empty()
        || !tx_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(UnderlayError::InvalidIntent(format!(
            "tx_id {tx_id:?} is invalid for file journal store"
        )));
    }
    Ok(())
}

fn journal_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("tx journal io error: {err}"))
}
