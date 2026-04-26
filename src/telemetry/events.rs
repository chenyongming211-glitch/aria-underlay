use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::tx::{TransactionStrategy, TxPhase};

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
