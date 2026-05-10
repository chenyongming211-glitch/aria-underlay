use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::intent::SwitchPairIntent;
use crate::model::{AclBinding, AclConfig, DeviceId, InterfaceConfig, VlanConfig};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDesiredState {
    pub device_id: DeviceId,
    pub vlans: BTreeMap<u16, VlanConfig>,
    pub interfaces: BTreeMap<String, InterfaceConfig>,
    #[serde(default)]
    pub acls: BTreeMap<u16, AclConfig>,
    #[serde(default)]
    pub acl_bindings: BTreeMap<String, AclBinding>,
    #[serde(default)]
    pub delete_vlan_ids: BTreeSet<u16>,
    #[serde(default)]
    pub delete_acl_ids: BTreeSet<u16>,
    #[serde(default)]
    pub delete_acl_bindings: BTreeMap<String, AclBinding>,
}

pub fn plan_switch_pair(intent: &SwitchPairIntent) -> Vec<DeviceDesiredState> {
    intent
        .switches
        .iter()
        .map(|switch| {
            let vlans = intent
                .vlans
                .iter()
                .map(|vlan| {
                    (
                        vlan.vlan_id,
                        VlanConfig {
                            vlan_id: vlan.vlan_id,
                            name: vlan.name.clone(),
                            description: vlan.description.clone(),
                        },
                    )
                })
                .collect();

            let interfaces = intent
                .interfaces
                .iter()
                .filter(|iface| iface.device_id == switch.device_id)
                .map(|iface| {
                    (
                        iface.name.clone(),
                        InterfaceConfig {
                            name: iface.name.clone(),
                            admin_state: iface.admin_state.clone(),
                            description: iface.description.clone(),
                            mode: iface.mode.clone(),
                        },
                    )
                })
                .collect();

            DeviceDesiredState {
                device_id: switch.device_id.clone(),
                vlans,
                interfaces,
                acls: BTreeMap::new(),
                acl_bindings: BTreeMap::new(),
                delete_vlan_ids: BTreeSet::new(),
                delete_acl_ids: BTreeSet::new(),
                delete_acl_bindings: BTreeMap::new(),
            }
        })
        .collect()
}

