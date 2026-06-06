use serde::{Deserialize, Serialize};

use crate::model::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpProcessIntent {
    pub device_id: DeviceId,
    pub vrf: String,
    pub local_as: u32,
    pub router_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpNeighborIntent {
    pub device_id: DeviceId,
    pub vrf: String,
    pub address: String,
    pub remote_as: u32,
    pub description: Option<String>,
    pub import_policy: Option<String>,
    pub export_policy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpProcessDeleteIntent {
    pub device_id: DeviceId,
    pub vrf: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpNeighborDeleteIntent {
    pub device_id: DeviceId,
    pub vrf: String,
    pub address: String,
}
