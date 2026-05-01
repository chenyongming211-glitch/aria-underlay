use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::audit::AuditRecord;
use crate::telemetry::events::{UnderlayEvent, UnderlayEventKind};
use crate::utils::atomic_file::atomic_write;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummaryRetentionPolicy {
    #[serde(default)]
    pub max_records: Option<usize>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default = "default_max_rotated_summary_files")]
    pub max_rotated_files: usize,
}

impl Default for OperationSummaryRetentionPolicy {
    fn default() -> Self {
        Self {
            max_records: None,
            max_bytes: None,
            max_rotated_files: default_max_rotated_summary_files(),
        }
    }
}

impl OperationSummaryRetentionPolicy {
    pub fn is_enabled(&self) -> bool {
        self.max_records.is_some() || self.max_bytes.is_some()
    }

    fn validate(&self) -> UnderlayResult<()> {
        if self.max_records == Some(0) {
            return Err(UnderlayError::InvalidIntent(
                "operation summary retention max_records must be greater than zero".into(),
            ));
        }
        if self.max_bytes == Some(0) {
            return Err(UnderlayError::InvalidIntent(
                "operation summary retention max_bytes must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummaryCompactionReport {
    pub compacted: bool,
    pub records_before: usize,
    pub records_after: usize,
    pub records_dropped: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub rotated_files: usize,
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

    pub fn compact(
        &self,
        policy: OperationSummaryRetentionPolicy,
    ) -> UnderlayResult<OperationSummaryCompactionReport> {
        policy.validate()?;
        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("operation summary file mutex poisoned".into())
        })?;
        if !self.path.exists() {
            return Ok(OperationSummaryCompactionReport::default());
        }

        let active_payload = fs::read(&self.path).map_err(summary_io_error)?;
        let active_text = String::from_utf8(active_payload.clone()).map_err(|err| {
            UnderlayError::Internal(format!(
                "parse operation summary {:?}: invalid utf-8: {err}",
                self.path
            ))
        })?;
        let summaries = parse_summary_lines(&self.path, &active_text)?;
        let retained = retain_summaries(&summaries, &policy)?;
        let retained_payload = encode_summaries(&retained)?;
        let records_before = summaries.len();
        let records_after = retained.len();
        let bytes_before = active_payload.len() as u64;
        let bytes_after = retained_payload.len() as u64;
        let records_dropped = records_before.saturating_sub(records_after);
        let size_reduced = policy
            .max_bytes
            .map(|max_bytes| bytes_before > max_bytes && bytes_after < bytes_before)
            .unwrap_or(false);
        let compacted = records_dropped > 0 || size_reduced;
        let mut report = OperationSummaryCompactionReport {
            compacted,
            records_before,
            records_after,
            records_dropped,
            bytes_before,
            bytes_after,
            rotated_files: 0,
        };
        if !compacted {
            return Ok(report);
        }

        report.rotated_files =
            self.rotate_active_payload(&active_payload, policy.max_rotated_files)?;
        atomic_write(&self.path, &retained_payload, summary_io_error)?;
        Ok(report)
    }

    fn rotate_active_payload(
        &self,
        active_payload: &[u8],
        max_rotated_files: usize,
    ) -> UnderlayResult<usize> {
        if max_rotated_files == 0 {
            return Ok(0);
        }

        for generation in (1..=max_rotated_files).rev() {
            let path = self.archive_path(generation);
            if generation == max_rotated_files {
                if path.exists() {
                    fs::remove_file(path).map_err(summary_io_error)?;
                }
                continue;
            }

            let next_path = self.archive_path(generation + 1);
            if path.exists() {
                fs::rename(path, next_path).map_err(summary_io_error)?;
            }
        }

        atomic_write(&self.archive_path(1), active_payload, summary_io_error)?;
        Ok(1)
    }

    fn archive_path(&self, generation: usize) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("operation-summaries.jsonl");
        self.path.with_file_name(format!("{file_name}.{generation}"))
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
        parse_summary_lines(&self.path, &payload)
    }
}

fn retain_summaries(
    summaries: &[OperationSummary],
    policy: &OperationSummaryRetentionPolicy,
) -> UnderlayResult<Vec<OperationSummary>> {
    let start = policy
        .max_records
        .map(|max_records| summaries.len().saturating_sub(max_records))
        .unwrap_or(0);
    let candidates = &summaries[start..];
    let Some(max_bytes) = policy.max_bytes else {
        return Ok(candidates.to_vec());
    };

    let mut retained = Vec::new();
    let mut bytes = 0_u64;
    for summary in candidates.iter().rev() {
        let record_bytes = encode_summary_line(summary)?.len() as u64;
        if bytes + record_bytes > max_bytes {
            break;
        }
        bytes += record_bytes;
        retained.push(summary.clone());
    }
    retained.reverse();
    Ok(retained)
}

fn parse_summary_lines(path: &Path, payload: &str) -> UnderlayResult<Vec<OperationSummary>> {
    let mut summaries = Vec::new();
    for (index, line) in payload.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let summary = serde_json::from_str::<OperationSummary>(line).map_err(|err| {
            UnderlayError::Internal(format!(
                "parse operation summary {:?} line {}: {err}",
                path,
                index + 1
            ))
        })?;
        summaries.push(summary);
    }
    Ok(summaries)
}

fn encode_summaries(summaries: &[OperationSummary]) -> UnderlayResult<Vec<u8>> {
    let mut payload = Vec::new();
    for summary in summaries {
        payload.extend(encode_summary_line(summary)?);
    }
    Ok(payload)
}

fn encode_summary_line(summary: &OperationSummary) -> UnderlayResult<Vec<u8>> {
    let mut payload = serde_json::to_vec(summary).map_err(|err| {
        UnderlayError::Internal(format!("serialize operation summary: {err}"))
    })?;
    payload.push(b'\n');
    Ok(payload)
}

fn is_operator_event(kind: &UnderlayEventKind) -> bool {
    matches!(
        kind,
        UnderlayEventKind::UnderlayDriftDetected
            | UnderlayEventKind::UnderlayDriftAuditCompleted
            | UnderlayEventKind::UnderlayJournalGcCompleted
            | UnderlayEventKind::UnderlayRecoveryCompleted
            | UnderlayEventKind::UnderlayAuditWriteFailed
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
        UnderlayEventKind::UnderlayAuditWriteFailed => true,
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

fn default_max_rotated_summary_files() -> usize {
    3
}
