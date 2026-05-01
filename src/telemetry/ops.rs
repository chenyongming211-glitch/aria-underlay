use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::audit::AuditRecord;
use crate::telemetry::events::{UnderlayEvent, UnderlayEventKind};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummary {
    pub request_id: String,
    pub trace_id: String,
    pub action: String,
    pub result: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub attention_required: bool,
    pub fields: BTreeMap<String, String>,
}

pub trait OperationSummaryStore: std::fmt::Debug + Send + Sync {
    fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()>;
    fn list(&self) -> UnderlayResult<Vec<OperationSummary>>;

    fn list_attention_required(&self) -> UnderlayResult<Vec<OperationSummary>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|summary| summary.attention_required)
            .collect())
    }
}

impl OperationSummary {
    pub fn from_event(event: &UnderlayEvent) -> Option<Self> {
        if !is_operator_event(&event.kind) {
            return None;
        }

        let audit = AuditRecord::from_event(event);
        Some(Self {
            request_id: audit.request_id,
            trace_id: audit.trace_id,
            action: audit.action,
            result: audit.result,
            tx_id: audit.tx_id,
            device_id: audit.device_id,
            attention_required: attention_required(event),
            fields: event.fields.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub struct InMemoryOperationSummaryStore {
    summaries: Mutex<Vec<OperationSummary>>,
}

impl InMemoryOperationSummaryStore {
    pub fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()> {
        <Self as OperationSummaryStore>::record_event(self, event)
    }

    pub fn list(&self) -> UnderlayResult<Vec<OperationSummary>> {
        <Self as OperationSummaryStore>::list(self)
    }

    pub fn list_attention_required(&self) -> UnderlayResult<Vec<OperationSummary>> {
        <Self as OperationSummaryStore>::list_attention_required(self)
    }
}

impl OperationSummaryStore for InMemoryOperationSummaryStore {
    fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()> {
        let Some(summary) = OperationSummary::from_event(event) else {
            return Ok(());
        };
        self.summaries
            .lock()
            .map_err(|_| UnderlayError::Internal("operation summary mutex poisoned".into()))?
            .push(summary);
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<OperationSummary>> {
        Ok(self
            .summaries
            .lock()
            .map_err(|_| UnderlayError::Internal("operation summary mutex poisoned".into()))?
            .clone())
    }
}

#[derive(Debug)]
pub struct JsonFileOperationSummaryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileOperationSummaryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()> {
        <Self as OperationSummaryStore>::record_event(self, event)
    }

    pub fn list(&self) -> UnderlayResult<Vec<OperationSummary>> {
        <Self as OperationSummaryStore>::list(self)
    }

    pub fn list_attention_required(&self) -> UnderlayResult<Vec<OperationSummary>> {
        <Self as OperationSummaryStore>::list_attention_required(self)
    }
}

impl OperationSummaryStore for JsonFileOperationSummaryStore {
    fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()> {
        let Some(summary) = OperationSummary::from_event(event) else {
            return Ok(());
        };

        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("operation summary file mutex poisoned".into())
        })?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(summary_io_error)?;
        }

        let mut payload = serde_json::to_vec(&summary).map_err(|err| {
            UnderlayError::Internal(format!("serialize operation summary: {err}"))
        })?;
        payload.push(b'\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(summary_io_error)?;
        file.write_all(&payload).map_err(summary_io_error)?;
        file.sync_all().map_err(summary_io_error)?;
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<OperationSummary>> {
        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("operation summary file mutex poisoned".into())
        })?;
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let payload = fs::read_to_string(&self.path).map_err(summary_io_error)?;
        let mut summaries = Vec::new();
        for (index, line) in payload.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let summary = serde_json::from_str::<OperationSummary>(line).map_err(|err| {
                UnderlayError::Internal(format!(
                    "parse operation summary {:?} line {}: {err}",
                    self.path,
                    index + 1
                ))
            })?;
            summaries.push(summary);
        }
        Ok(summaries)
    }
}

fn is_operator_event(kind: &UnderlayEventKind) -> bool {
    matches!(
        kind,
        UnderlayEventKind::UnderlayDriftDetected
            | UnderlayEventKind::UnderlayDriftAuditCompleted
            | UnderlayEventKind::UnderlayJournalGcCompleted
            | UnderlayEventKind::UnderlayRecoveryCompleted
            | UnderlayEventKind::UnderlayTransactionForceResolved
            | UnderlayEventKind::UnderlayTransactionInDoubt
    )
}

fn attention_required(event: &UnderlayEvent) -> bool {
    match &event.kind {
        UnderlayEventKind::UnderlayDriftDetected => true,
        UnderlayEventKind::UnderlayDriftAuditCompleted => {
            event.result.as_deref() == Some("drift_detected")
        }
        UnderlayEventKind::UnderlayRecoveryCompleted => {
            field_value(event, "in_doubt") > 0 || field_value(event, "pending") > 0
        }
        UnderlayEventKind::UnderlayTransactionInDoubt => true,
        _ => false,
    }
}

fn field_value(event: &UnderlayEvent, name: &str) -> u64 {
    event
        .fields
        .get(name)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
}

fn summary_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("operation summary io error: {err}"))
}
