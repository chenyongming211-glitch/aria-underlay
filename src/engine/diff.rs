use serde::{Deserialize, Serialize};

use crate::engine::normalize::{normalize_desired_state, normalize_shadow_state};
use crate::model::{DeviceId, InterfaceConfig, VlanConfig};
use crate::planner::device_plan::DeviceDesiredState;
use crate::state::DeviceShadowState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub device_id: DeviceId,
    pub ops: Vec<ChangeOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

pub fn compute_diff(desired: &DeviceDesiredState, current: &DeviceShadowState) -> ChangeSet {
    let desired = normalize_desired_state(desired.clone());
    let current = normalize_shadow_state(current.clone());
    let mut change_set = ChangeSet::empty(desired.device_id.clone());

    for (vlan_id, desired_vlan) in &desired.vlans {
        match current.vlans.get(vlan_id) {
            Some(current_vlan) if current_vlan == desired_vlan => {}
            Some(current_vlan) => change_set.ops.push(ChangeOp::UpdateVlan {
                before: current_vlan.clone(),
                after: desired_vlan.clone(),
            }),
            None => change_set
                .ops
                .push(ChangeOp::CreateVlan(desired_vlan.clone())),
        }
    }

    for vlan_id in current.vlans.keys() {
        if !desired.vlans.contains_key(vlan_id) {
            change_set.ops.push(ChangeOp::DeleteVlan { vlan_id: *vlan_id });
        }
    }

    for (name, desired_interface) in &desired.interfaces {
        match current.interfaces.get(name) {
            Some(current_interface) if current_interface == desired_interface => {}
            Some(current_interface) => change_set.ops.push(ChangeOp::UpdateInterface {
                before: Some(current_interface.clone()),
                after: desired_interface.clone(),
            }),
            None => change_set.ops.push(ChangeOp::UpdateInterface {
                before: None,
                after: desired_interface.clone(),
            }),
        }
    }

    for name in current.interfaces.keys() {
        if !desired.interfaces.contains_key(name) {
            change_set
                .ops
                .push(ChangeOp::DeleteInterfaceConfig { name: name.clone() });
        }
    }

    change_set
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
