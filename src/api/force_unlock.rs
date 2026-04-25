use serde::{Deserialize, Serialize};

use crate::model::DeviceId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForceUnlockRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub device_id: DeviceId,
    pub lock_owner: String,
    pub reason: String,
    pub break_glass_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForceUnlockResponse {
    pub device_id: DeviceId,
    pub unlocked: bool,
    pub warnings: Vec<String>,
}

