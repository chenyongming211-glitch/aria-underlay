use serde::{Deserialize, Serialize};

use crate::intent::{interface::InterfaceIntent, vlan::VlanIntent};
use crate::model::{DeviceId, DeviceRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchPairIntent {
    pub pair_id: String,
    pub switches: Vec<SwitchIntent>,
    pub vlans: Vec<VlanIntent>,
    pub interfaces: Vec<InterfaceIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchIntent {
    pub device_id: DeviceId,
    pub role: DeviceRole,
}

