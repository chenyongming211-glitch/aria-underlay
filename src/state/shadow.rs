use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{DeviceId, InterfaceConfig, VlanConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceShadowState {
    pub device_id: DeviceId,
    pub revision: u64,
    pub vlans: BTreeMap<u16, VlanConfig>,
    pub interfaces: BTreeMap<String, InterfaceConfig>,
    pub warnings: Vec<String>,
}

