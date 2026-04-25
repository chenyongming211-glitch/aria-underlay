use serde::{Deserialize, Serialize};

use crate::model::{DeviceId, InterfaceConfig, VlanConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    pub device_id: DeviceId,
    pub ops: Vec<ChangeOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeOp {
    CreateVlan(VlanConfig),
    UpdateVlan {
        before: VlanConfig,
        after: VlanConfig,
    },
    DeleteVlan {
        vlan_id: u16,
    },
    UpdateInterface {
        before: Option<InterfaceConfig>,
        after: InterfaceConfig,
    },
    DeleteInterfaceConfig {
        name: String,
    },
}

impl ChangeSet {
    pub fn empty(device_id: DeviceId) -> Self {
        Self {
            device_id,
            ops: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

