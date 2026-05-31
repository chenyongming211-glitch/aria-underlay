use serde::{Deserialize, Serialize};

use crate::intent::{SwitchPairIntent, UnderlayDomainIntent};
use crate::model::DeviceId;
use crate::state::drift::DriftPolicy;

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
#[serde(deny_unknown_fields)]
pub struct ApplyOptions {
    pub dry_run: bool,
    pub allow_degraded_atomicity: bool,
    #[serde(default)]
    pub drift_policy: DriftPolicy,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            allow_degraded_atomicity: false,
            drift_policy: DriftPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshStateRequest {
    pub device_ids: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAuditRequest {
    pub device_ids: Vec<DeviceId>,
}
