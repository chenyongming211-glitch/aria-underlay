use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::model::DeviceId;
use crate::telemetry::alerts::OperationAlertLifecycleStatus;
use crate::telemetry::events::{UnderlayEvent, UnderlayEventKind};
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub request_id: String,
    pub trace_id: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub action: String,
    pub result: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAuditRecord {
    pub appended_at_unix_secs: u64,
    pub request_id: String,
    pub trace_id: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub action: String,
    pub result: String,
    pub operator_id: Option<String>,
    pub reason: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAuditRetentionPolicy {
    #[serde(default)]
    pub max_records: Option<usize>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default = "default_max_rotated_audit_files")]
    pub max_rotated_files: usize,
}

impl Default for OperationAuditRetentionPolicy {
    fn default() -> Self {
        Self {
            max_records: None,
            max_bytes: None,
            max_rotated_files: default_max_rotated_audit_files(),
        }
    }
}

impl OperationAuditRetentionPolicy {
    pub fn is_enabled(&self) -> bool {
        self.max_records.is_some() || self.max_bytes.is_some()
    }

    pub fn validate(&self) -> UnderlayResult<()> {
        if self.max_records == Some(0) {
            return Err(UnderlayError::InvalidIntent(
                "operation audit retention max_records must be greater than zero".into(),
            ));
        }
        if self.max_bytes == Some(0) {
            return Err(UnderlayError::InvalidIntent(
                "operation audit retention max_bytes must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAuditCompactionReport {
    pub compacted: bool,
    pub records_before: usize,
    pub records_after: usize,
    pub records_dropped: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub rotated_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductAuditRecord {
    pub request_id: String,
    pub trace_id: String,
    pub action: String,
    pub result: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub operator_id: Option<String>,
    pub reason: Option<String>,
    pub attention_required: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub appended_at_unix_secs: u64,
}

impl ProductAuditRecord {
    pub fn force_resolve_requested(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        tx_id: impl Into<String>,
        operator_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            action: "transaction.force_resolve_requested".into(),
            result: "authorized".into(),
            tx_id: Some(tx_id.into()),
            device_id: None,
            operator_id: Some(operator_id.into()),
            reason: Some(reason.into()),
            attention_required: false,
            error_code: None,
            error_message: None,
            fields: BTreeMap::new(),
            appended_at_unix_secs: now_unix_secs(),
        }
    }

    pub fn alert_lifecycle_transition(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        dedupe_key: impl Into<String>,
        status: OperationAlertLifecycleStatus,
        operator_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let dedupe_key = dedupe_key.into();
        let status_label = format!("{status:?}");
        let mut fields = BTreeMap::new();
        fields.insert("dedupe_key".into(), dedupe_key);
        fields.insert("status".into(), status_label);

        Self {
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            action: alert_lifecycle_action(&status).into(),
            result: "authorized".into(),
            tx_id: None,
            device_id: None,
            operator_id: Some(operator_id.into()),
            reason: Some(reason.into()),
            attention_required: false,
            error_code: None,
            error_message: None,
            fields,
            appended_at_unix_secs: now_unix_secs(),
        }
    }

    pub fn worker_config_change_requested(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        action: impl Into<String>,
        target: impl Into<String>,
        operator_id: impl Into<String>,
        reason: impl Into<String>,
        mut fields: BTreeMap<String, String>,
    ) -> Self {
        fields.insert("target".into(), target.into());

        Self {
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            action: action.into(),
            result: "authorized".into(),
            tx_id: None,
            device_id: None,
            operator_id: Some(operator_id.into()),
            reason: Some(reason.into()),
            attention_required: false,
            error_code: None,
            error_message: None,
            fields,
            appended_at_unix_secs: now_unix_secs(),
        }
    }

    pub fn product_audit_export_requested(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        operator_id: impl Into<String>,
        reason: impl Into<String>,
        mut fields: BTreeMap<String, String>,
    ) -> Self {
        fields.insert("export_type".into(), "product_audit".into());

        Self {
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            action: "product_audit.export_requested".into(),
            result: "authorized".into(),
            tx_id: None,
            device_id: None,
            operator_id: Some(operator_id.into()),
            reason: Some(reason.into()),
            attention_required: false,
            error_code: None,
            error_message: None,
            fields,
            appended_at_unix_secs: now_unix_secs(),
        }
    }
}

impl OperationAuditRecord {
    pub fn from_event(event: &UnderlayEvent) -> Self {
        let audit = AuditRecord::from_event(event);
        Self {
            appended_at_unix_secs: now_unix_secs(),
            request_id: audit.request_id,
            trace_id: audit.trace_id,
            tx_id: audit.tx_id,
            device_id: audit.device_id,
            action: audit.action,
            result: audit.result,
            operator_id: event
                .fields
                .get("operator")
                .or_else(|| event.fields.get("operator_id"))
                .cloned(),
            reason: event.fields.get("reason").cloned(),
            error_code: audit.error_code,
            error_message: audit.error_message,
            fields: event.fields.clone(),
        }
    }
}

pub trait OperationAuditStore: std::fmt::Debug + Send + Sync {
    fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()>;
    fn list(&self) -> UnderlayResult<Vec<OperationAuditRecord>>;
}

#[derive(Debug, Default)]
pub struct NoopOperationAuditStore;

impl OperationAuditStore for NoopOperationAuditStore {
    fn record_event(&self, _event: &UnderlayEvent) -> UnderlayResult<()> {
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<OperationAuditRecord>> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
pub struct JsonFileOperationAuditStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileOperationAuditStore {
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
        <Self as OperationAuditStore>::record_event(self, event)
    }

    pub fn list(&self) -> UnderlayResult<Vec<OperationAuditRecord>> {
        <Self as OperationAuditStore>::list(self)
    }

    pub fn compact(
        &self,
        policy: OperationAuditRetentionPolicy,
    ) -> UnderlayResult<OperationAuditCompactionReport> {
        policy.validate()?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("operation audit file mutex poisoned".into()))?;
        if !self.path.exists() {
            return Ok(OperationAuditCompactionReport::default());
        }

        let active_payload = fs::read(&self.path).map_err(operation_audit_io_error)?;
        let active_text = String::from_utf8(active_payload.clone()).map_err(|err| {
            UnderlayError::Internal(format!(
                "parse operation audit {:?}: invalid utf-8: {err}",
                self.path
            ))
        })?;
        let records = parse_operation_audit_lines(&self.path, &active_text)?;
        let retained = retain_operation_audit_records(&records, &policy)?;
        let retained_payload = encode_operation_audit_records(&retained)?;
        let records_before = records.len();
        let records_after = retained.len();
        let bytes_before = active_payload.len() as u64;
        let bytes_after = retained_payload.len() as u64;
        let records_dropped = records_before.saturating_sub(records_after);
        let size_reduced = policy
            .max_bytes
            .map(|max_bytes| bytes_before > max_bytes && bytes_after < bytes_before)
            .unwrap_or(false);
        let compacted = records_dropped > 0 || size_reduced;
        let mut report = OperationAuditCompactionReport {
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
        crate::utils::atomic_file::atomic_write(
            &self.path,
            &retained_payload,
            operation_audit_io_error,
        )?;
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
                    fs::remove_file(path).map_err(operation_audit_io_error)?;
                }
                continue;
            }

            let next_path = self.archive_path(generation + 1);
            if path.exists() {
                fs::rename(path, next_path).map_err(operation_audit_io_error)?;
            }
        }

        crate::utils::atomic_file::atomic_write(
            &self.archive_path(1),
            active_payload,
            operation_audit_io_error,
        )?;
        Ok(1)
    }

    fn archive_path(&self, generation: usize) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("operation-audit.jsonl");
        self.path.with_file_name(format!("{file_name}.{generation}"))
    }
}

impl OperationAuditStore for JsonFileOperationAuditStore {
    fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()> {
        let record = OperationAuditRecord::from_event(event);
        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("operation audit file mutex poisoned".into()))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(operation_audit_io_error)?;
        }

        let mut payload = serde_json::to_vec(&record).map_err(|err| {
            UnderlayError::Internal(format!("serialize operation audit: {err}"))
        })?;
        payload.push(b'\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(operation_audit_io_error)?;
        file.write_all(&payload).map_err(operation_audit_io_error)?;
        file.sync_all().map_err(operation_audit_io_error)?;
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<OperationAuditRecord>> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("operation audit file mutex poisoned".into()))?;
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let payload = fs::read_to_string(&self.path).map_err(operation_audit_io_error)?;
        parse_operation_audit_lines(&self.path, &payload)
    }
}

pub trait ProductAuditStore: std::fmt::Debug + Send + Sync {
    fn append(&self, record: ProductAuditRecord) -> UnderlayResult<()>;
    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>>;
}

#[derive(Debug, Default)]
pub struct NoopProductAuditStore;

impl ProductAuditStore for NoopProductAuditStore {
    fn append(&self, _record: ProductAuditRecord) -> UnderlayResult<()> {
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryProductAuditStore {
    records: Mutex<Vec<ProductAuditRecord>>,
}

impl InMemoryProductAuditStore {
    pub fn records(&self) -> Vec<ProductAuditRecord> {
        self.records
            .lock()
            .expect("in-memory product audit store mutex should not be poisoned")
            .clone()
    }
}

impl ProductAuditStore for InMemoryProductAuditStore {
    fn append(&self, record: ProductAuditRecord) -> UnderlayResult<()> {
        self.records
            .lock()
            .map_err(|_| UnderlayError::Internal("product audit store mutex poisoned".into()))?
            .push(record);
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        Ok(self.records())
    }
}

#[derive(Debug)]
pub struct JsonFileProductAuditStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileProductAuditStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("product audit file mutex poisoned".into()))?;
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let payload = fs::read_to_string(&self.path).map_err(product_audit_io_error)?;
        parse_product_audit_lines(&self.path, &payload)
    }
}

impl ProductAuditStore for JsonFileProductAuditStore {
    fn append(&self, record: ProductAuditRecord) -> UnderlayResult<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("product audit file mutex poisoned".into()))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(product_audit_io_error)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(product_audit_io_error)?;
        let mut payload = serde_json::to_vec(&record)
            .map_err(|err| UnderlayError::Internal(format!("serialize product audit: {err}")))?;
        payload.push(b'\n');
        file.write_all(&payload).map_err(product_audit_io_error)?;
        file.sync_all().map_err(product_audit_io_error)?;
        Ok(())
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        JsonFileProductAuditStore::list(self)
    }
}

impl AuditRecord {
    pub fn from_event(event: &UnderlayEvent) -> Self {
        Self {
            request_id: event.request_id.clone(),
            trace_id: event.trace_id.clone(),
            tx_id: event.tx_id.clone(),
            device_id: event.device_id.clone(),
            action: action_name(&event.kind).into(),
            result: event
                .result
                .clone()
                .unwrap_or_else(|| "observed".into()),
            error_code: event.error_code.clone(),
            error_message: event.error_message.clone(),
        }
    }
}

fn action_name(kind: &UnderlayEventKind) -> &'static str {
    match kind {
        UnderlayEventKind::UnderlayDeviceRegistered => "device.registered",
        UnderlayEventKind::UnderlayDeviceCapabilityDetected => "device.capability_detected",
        UnderlayEventKind::UnderlayDriftDetected => "drift.detected",
        UnderlayEventKind::UnderlayDriftAuditCompleted => "drift.audit_completed",
        UnderlayEventKind::UnderlayDeviceLockTimeout => "device.lock_timeout",
        UnderlayEventKind::UnderlayForceUnlockRequested => "device.force_unlock_requested",
        UnderlayEventKind::UnderlayJournalGcCompleted => "journal.gc_completed",
        UnderlayEventKind::UnderlayRecoveryCompleted => "recovery.completed",
        UnderlayEventKind::UnderlayAuditWriteFailed => "audit.write_failed",
        UnderlayEventKind::UnderlayTransactionStarted => "transaction.started",
        UnderlayEventKind::UnderlayTransactionPhaseChanged => "transaction.phase_changed",
        UnderlayEventKind::UnderlayTransactionCompleted => "transaction.completed",
        UnderlayEventKind::UnderlayTransactionFailed => "transaction.failed",
        UnderlayEventKind::UnderlayTransactionInDoubt => "transaction.in_doubt",
        UnderlayEventKind::UnderlayTransactionForceResolved => "transaction.force_resolved",
    }
}

fn alert_lifecycle_action(status: &OperationAlertLifecycleStatus) -> &'static str {
    match status {
        OperationAlertLifecycleStatus::Open => "alert.opened",
        OperationAlertLifecycleStatus::Acknowledged => "alert.acknowledged",
        OperationAlertLifecycleStatus::Resolved => "alert.resolved",
        OperationAlertLifecycleStatus::Suppressed => "alert.suppressed",
        OperationAlertLifecycleStatus::Expired => "alert.expired",
    }
}

fn parse_product_audit_lines(
    path: &Path,
    payload: &str,
) -> UnderlayResult<Vec<ProductAuditRecord>> {
    let mut records = Vec::new();
    for (index, line) in payload.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<ProductAuditRecord>(line).map_err(|err| {
            UnderlayError::ProductAuditWriteFailed(format!(
                "parse product audit {:?} line {}: {err}",
                path,
                index + 1
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

fn retain_operation_audit_records(
    records: &[OperationAuditRecord],
    policy: &OperationAuditRetentionPolicy,
) -> UnderlayResult<Vec<OperationAuditRecord>> {
    let start = policy
        .max_records
        .map(|max_records| records.len().saturating_sub(max_records))
        .unwrap_or(0);
    let candidates = &records[start..];
    let Some(max_bytes) = policy.max_bytes else {
        return Ok(candidates.to_vec());
    };

    let mut retained = Vec::new();
    let mut bytes = 0_u64;
    for record in candidates.iter().rev() {
        let record_bytes = encode_operation_audit_line(record)?.len() as u64;
        if bytes + record_bytes > max_bytes {
            break;
        }
        bytes += record_bytes;
        retained.push(record.clone());
    }
    retained.reverse();
    Ok(retained)
}

fn parse_operation_audit_lines(
    path: &Path,
    payload: &str,
) -> UnderlayResult<Vec<OperationAuditRecord>> {
    let mut records = Vec::new();
    for (index, line) in payload.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<OperationAuditRecord>(line).map_err(|err| {
            UnderlayError::Internal(format!(
                "parse operation audit {:?} line {}: {err}",
                path,
                index + 1
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

fn encode_operation_audit_records(records: &[OperationAuditRecord]) -> UnderlayResult<Vec<u8>> {
    let mut payload = Vec::new();
    for record in records {
        payload.extend(encode_operation_audit_line(record)?);
    }
    Ok(payload)
}

fn encode_operation_audit_line(record: &OperationAuditRecord) -> UnderlayResult<Vec<u8>> {
    let mut payload = serde_json::to_vec(record).map_err(|err| {
        UnderlayError::Internal(format!("serialize operation audit: {err}"))
    })?;
    payload.push(b'\n');
    Ok(payload)
}

fn operation_audit_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("operation audit io error: {err}"))
}

fn product_audit_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::ProductAuditWriteFailed(format!("product audit io error: {err}"))
}

fn default_max_rotated_audit_files() -> usize {
    3
}
