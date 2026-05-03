use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::authz::RbacRole;
use crate::model::DeviceId;
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
}

pub trait ProductAuditStore: std::fmt::Debug + Send + Sync {
    fn append(&self, record: ProductAuditRecord) -> UnderlayResult<()>;
}

#[derive(Debug, Default)]
pub struct NoopProductAuditStore;

impl ProductAuditStore for NoopProductAuditStore {
    fn append(&self, _record: ProductAuditRecord) -> UnderlayResult<()> {
        Ok(())
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
