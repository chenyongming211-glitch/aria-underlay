use serde::{Deserialize, Serialize};

use crate::intent::{SwitchPairIntent, UnderlayDomainIntent};
use crate::model::DeviceId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyIntentRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub intent: SwitchPairIntent,
    pub options: ApplyOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyDomainIntentRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub intent: UnderlayDomainIntent,
    pub options: ApplyOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyOptions {
    pub dry_run: bool,
    pub allow_degraded_atomicity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshStateRequest {
    pub device_ids: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAuditRequest {
    pub device_ids: Vec<DeviceId>,
}
