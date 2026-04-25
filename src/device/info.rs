use serde::{Deserialize, Serialize};

use crate::model::{DeviceId, DeviceRole, Vendor};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostKeyPolicy {
    TrustOnFirstUse,
    KnownHostsFile { path: String },
    PinnedKey { fingerprint: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceLifecycleState {
    Pending,
    Probing,
    Ready,
    Degraded,
    Unsupported,
    Unreachable,
    AuthFailed,
    Drifted,
    Maintenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub tenant_id: String,
    pub site_id: String,
    pub id: DeviceId,
    pub management_ip: String,
    pub management_port: u16,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
    pub role: DeviceRole,
    pub secret_ref: String,
    pub host_key_policy: HostKeyPolicy,
    pub adapter_endpoint: String,
    pub lifecycle_state: DeviceLifecycleState,
}

