use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::authz::RbacRole;
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
pub struct ProductAuditRecord {
    pub request_id: String,
    pub trace_id: String,
    pub action: String,
    pub result: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub operator_id: Option<String>,
    pub role: Option<RbacRole>,
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
        role: RbacRole,
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
            role: Some(role),
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
        role: RbacRole,
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
            role: Some(role),
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
        role: RbacRole,
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
            role: Some(role),
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
        role: RbacRole,
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
            role: Some(role),
            reason: Some(reason.into()),
            attention_required: false,
            error_code: None,
            error_message: None,
            fields,
            appended_at_unix_secs: now_unix_secs(),
        }
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

fn product_audit_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::ProductAuditWriteFailed(format!("product audit io error: {err}"))
}
