use serde::{Deserialize, Serialize};

use crate::engine::normalize::{normalize_desired_state, normalize_shadow_state};
use crate::model::{
    acl_binding_key, AclBinding, AclConfig, AclDirection, DeviceId, InterfaceConfig, VlanConfig,
};
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
    CreateAcl(AclConfig),
    UpdateAcl {
        before: AclConfig,
        after: AclConfig,
    },
    DeleteAcl {
        acl_id: u16,
    },
    CreateAclBinding(AclBinding),
    UpdateAclBinding {
        before: AclBinding,
        after: AclBinding,
    },
    DeleteAclBinding {
        interface_name: String,
        direction: AclDirection,
        acl_id: u16,
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

    for vlan_id in &desired.delete_vlan_ids {
        if current.vlans.contains_key(vlan_id) {
            change_set.ops.push(ChangeOp::DeleteVlan { vlan_id: *vlan_id });
        }
    }

    for (acl_id, desired_acl) in &desired.acls {
        match current.acls.get(acl_id) {
            Some(current_acl) if current_acl == desired_acl => {}
            Some(current_acl) => change_set.ops.push(ChangeOp::UpdateAcl {
                before: current_acl.clone(),
                after: desired_acl.clone(),
            }),
            None => change_set.ops.push(ChangeOp::CreateAcl(desired_acl.clone())),
        }
    }

    for (key, desired_binding) in &desired.acl_bindings {
        match current.acl_bindings.get(key) {
            Some(current_binding) if current_binding == desired_binding => {}
            Some(current_binding) => change_set.ops.push(ChangeOp::UpdateAclBinding {
                before: current_binding.clone(),
                after: desired_binding.clone(),
            }),
            None => change_set
                .ops
                .push(ChangeOp::CreateAclBinding(desired_binding.clone())),
        }
    }

    for (key, delete_binding) in &desired.delete_acl_bindings {
        if current
            .acl_bindings
            .get(key)
            .is_some_and(|current_binding| current_binding.acl_id == delete_binding.acl_id)
        {
            change_set.ops.push(ChangeOp::DeleteAclBinding {
                interface_name: delete_binding.interface_name.clone(),
                direction: delete_binding.direction.clone(),
                acl_id: delete_binding.acl_id,
            });
        }
    }

    for acl_id in &desired.delete_acl_ids {
        if current.acls.contains_key(acl_id) {
            change_set.ops.push(ChangeOp::DeleteAcl { acl_id: *acl_id });
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

    pub fn apply_to_shadow(
        &self,
        base: Option<&DeviceShadowState>,
        desired: &DeviceDesiredState,
        revision: u64,
    ) -> DeviceShadowState {
        let mut state = base.cloned().unwrap_or_else(|| DeviceShadowState {
            device_id: desired.device_id.clone(),
            revision,
            vlans: Default::default(),
            interfaces: Default::default(),
            acls: Default::default(),
            acl_bindings: Default::default(),
            warnings: Vec::new(),
        });
        state.device_id = desired.device_id.clone();
        state.revision = revision;
        state.warnings.clear();

        for op in &self.ops {
            match op {
                ChangeOp::CreateVlan(vlan) => {
                    state.vlans.insert(vlan.vlan_id, vlan.clone());
                }
                ChangeOp::UpdateVlan { after, .. } => {
                    state.vlans.insert(after.vlan_id, after.clone());
                }
                ChangeOp::DeleteVlan { vlan_id } => {
                    state.vlans.remove(vlan_id);
                }
                ChangeOp::UpdateInterface { after, .. } => {
                    state.interfaces.insert(after.name.clone(), after.clone());
                }
                ChangeOp::CreateAcl(acl) => {
                    state.acls.insert(acl.acl_id, acl.clone());
                }
                ChangeOp::UpdateAcl { after, .. } => {
                    state.acls.insert(after.acl_id, after.clone());
                }
                ChangeOp::DeleteAcl { acl_id } => {
                    state.acls.remove(acl_id);
                }
                ChangeOp::CreateAclBinding(binding) => {
                    state.acl_bindings.insert(binding.key(), binding.clone());
                }
                ChangeOp::UpdateAclBinding { after, .. } => {
                    state.acl_bindings.insert(after.key(), after.clone());
                }
                ChangeOp::DeleteAclBinding {
                    interface_name,
                    direction,
                    ..
                } => {
                    state
                        .acl_bindings
                        .remove(&acl_binding_key(interface_name, direction));
                }
            }
        }

        state
    }
}
