use serde::{Deserialize, Serialize};

use crate::device::DeviceLifecycleState;
use crate::engine::diff::ChangeSet;
use crate::model::DeviceId;
use crate::tx::TransactionStrategy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyStatus {
    NoOpSuccess,
    Success,
    SuccessWithWarning,
    Failed,
    RolledBack,
    InDoubt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceApplyResult {
    pub device_id: DeviceId,
    pub changed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyIntentResponse {
    pub request_id: String,
    pub trace_id: String,
    pub tx_id: Option<String>,
    pub status: ApplyStatus,
    pub strategy: Option<TransactionStrategy>,
    pub device_results: Vec<DeviceApplyResult>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResponse {
    pub device_results: Vec<DeviceApplyResult>,
    pub change_sets: Vec<ChangeSet>,
    pub noop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshStateResponse {
    pub refreshed_devices: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceOnboardingResponse {
    pub device_id: DeviceId,
    pub lifecycle_state: DeviceLifecycleState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAuditResponse {
    pub drifted_devices: Vec<DeviceId>,
}
