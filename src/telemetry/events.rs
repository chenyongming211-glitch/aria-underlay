use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::response::{ApplyStatus, DeviceApplyResult};
use crate::model::DeviceId;
use crate::state::drift::DriftReport;
use crate::tx::{TransactionStrategy, TxPhase};
use crate::worker::gc::JournalGcReport;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnderlayEventKind {
    UnderlayDeviceRegistered,
    UnderlayDeviceCapabilityDetected,
    UnderlayDriftDetected,
    UnderlayDeviceLockTimeout,
    UnderlayForceUnlockRequested,
    UnderlayJournalGcCompleted,
    UnderlayTransactionStarted,
    UnderlayTransactionPhaseChanged,
    UnderlayTransactionCompleted,
    UnderlayTransactionFailed,
    UnderlayTransactionInDoubt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnderlayEvent {
    pub kind: UnderlayEventKind,
    pub request_id: String,
    pub trace_id: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub phase: Option<TxPhase>,
    pub strategy: Option<TransactionStrategy>,
    pub result: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub fields: BTreeMap<String, String>,
}

impl UnderlayEvent {
    pub fn from_device_apply_result(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        result: &DeviceApplyResult,
    ) -> Option<Self> {
        let tx_id = result.tx_id.clone()?;
        let phase = tx_phase_for_apply_status(&result.status)?;
        let mut event = Self::transaction_result(
            request_id,
            trace_id,
            tx_id,
            Some(result.device_id.clone()),
            phase,
            result.strategy,
            apply_result_name(&result.status),
        );
        if let (Some(code), Some(message)) = (&result.error_code, &result.error_message) {
            event = event.with_error(code.clone(), message.clone());
        }
        if !result.warnings.is_empty() {
            event
                .fields
                .insert("warning_count".into(), result.warnings.len().to_string());
        }
        Some(event)
    }

    pub fn drift_detected(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        report: &DriftReport,
    ) -> Self {
        let mut fields = BTreeMap::new();
        fields.insert("finding_count".into(), report.findings.len().to_string());
        fields.insert("warning_count".into(), report.warnings.len().to_string());
        if let Some(first) = report.findings.first() {
            fields.insert("first_drift_type".into(), format!("{:?}", first.drift_type));
            fields.insert("first_path".into(), first.path.clone());
        }

        Self {
            kind: UnderlayEventKind::UnderlayDriftDetected,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            tx_id: None,
            device_id: Some(report.device_id.clone()),
            phase: None,
            strategy: None,
            result: Some(if report.drift_detected {
                "drift_detected".into()
            } else {
                "clean".into()
            }),
            error_code: None,
            error_message: None,
            fields,
        }
    }

    pub fn journal_gc_completed(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        report: &JournalGcReport,
    ) -> Self {
        let mut fields = BTreeMap::new();
        fields.insert(
            "journals_deleted".into(),
            report.journals_deleted.to_string(),
        );
        fields.insert(
            "journals_retained".into(),
            report.journals_retained.to_string(),
        );
        fields.insert(
            "artifacts_deleted".into(),
            report.artifacts_deleted.to_string(),
        );

        Self {
            kind: UnderlayEventKind::UnderlayJournalGcCompleted,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            tx_id: None,
            device_id: None,
            phase: None,
            strategy: None,
            result: Some("completed".into()),
            error_code: None,
            error_message: None,
            fields,
        }
    }

    pub fn transaction_phase(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        tx_id: impl Into<String>,
        device_id: Option<DeviceId>,
        phase: TxPhase,
        strategy: Option<TransactionStrategy>,
    ) -> Self {
        Self {
            kind: UnderlayEventKind::UnderlayTransactionPhaseChanged,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            tx_id: Some(tx_id.into()),
            device_id,
            phase: Some(phase),
            strategy,
            result: None,
            error_code: None,
            error_message: None,
            fields: BTreeMap::new(),
        }
    }

    pub fn transaction_result(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        tx_id: impl Into<String>,
        device_id: Option<DeviceId>,
        phase: TxPhase,
        strategy: Option<TransactionStrategy>,
        result: impl Into<String>,
    ) -> Self {
        let result = result.into();
        let kind = if phase == TxPhase::InDoubt {
            UnderlayEventKind::UnderlayTransactionInDoubt
        } else if matches!(phase, TxPhase::Committed | TxPhase::RolledBack) {
            UnderlayEventKind::UnderlayTransactionCompleted
        } else {
            UnderlayEventKind::UnderlayTransactionFailed
        };

        Self {
            kind,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            tx_id: Some(tx_id.into()),
            device_id,
            phase: Some(phase),
            strategy,
            result: Some(result),
            error_code: None,
            error_message: None,
            fields: BTreeMap::new(),
        }
    }

    pub fn with_error(
        mut self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.error_code = Some(code.into());
        self.error_message = Some(message.into());
        self
    }
}

fn tx_phase_for_apply_status(status: &ApplyStatus) -> Option<TxPhase> {
    match status {
        ApplyStatus::Success | ApplyStatus::SuccessWithWarning => Some(TxPhase::Committed),
        ApplyStatus::RolledBack => Some(TxPhase::RolledBack),
        ApplyStatus::InDoubt => Some(TxPhase::InDoubt),
        ApplyStatus::Failed => Some(TxPhase::Failed),
        ApplyStatus::NoOpSuccess => None,
    }
}

fn apply_result_name(status: &ApplyStatus) -> &'static str {
    match status {
        ApplyStatus::NoOpSuccess => "no_op_success",
        ApplyStatus::Success => "success",
        ApplyStatus::SuccessWithWarning => "success_with_warning",
        ApplyStatus::Failed => "failed",
        ApplyStatus::RolledBack => "rolled_back",
        ApplyStatus::InDoubt => "in_doubt",
    }
}
