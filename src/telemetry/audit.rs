use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::events::{UnderlayEvent, UnderlayEventKind};

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
