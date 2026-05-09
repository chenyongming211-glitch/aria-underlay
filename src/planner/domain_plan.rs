use std::collections::BTreeMap;

use crate::intent::validation::validate_underlay_domain_intent;
use crate::intent::UnderlayDomainIntent;
use crate::model::{AclBinding, AclConfig, DeviceId, InterfaceConfig, VlanConfig};
use crate::planner::device_plan::DeviceDesiredState;
use crate::{UnderlayError, UnderlayResult};

pub fn plan_underlay_domain(
    intent: &UnderlayDomainIntent,
) -> UnderlayResult<Vec<DeviceDesiredState>> {
    validate_underlay_domain_intent(intent)?;

    let member_to_endpoint = intent
        .members
        .iter()
        .map(|member| {
            (
                DeviceId(member.member_id.clone()),
                DeviceId(member.management_endpoint_id.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut states = intent
        .endpoints
        .iter()
        .map(|endpoint| {
            (
                DeviceId(endpoint.endpoint_id.clone()),
                DeviceDesiredState {
                    device_id: DeviceId(endpoint.endpoint_id.clone()),
                    vlans: intent
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
                        .collect(),
                    interfaces: BTreeMap::new(),
                    acls: intent
                        .acls
                        .iter()
                        .map(|acl| {
                            (
                                acl.acl_id,
                                AclConfig {
                                    acl_id: acl.acl_id,
                                    name: acl.name.clone(),
                                    description: acl.description.clone(),
                                    rules: acl.rules.clone(),
                                },
                            )
                        })
                        .collect(),
                    acl_bindings: BTreeMap::new(),
                    delete_vlan_ids: intent.delete_vlan_ids.iter().copied().collect(),
                    delete_acl_ids: intent.delete_acl_ids.iter().copied().collect(),
                    delete_acl_bindings: BTreeMap::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    for interface in &intent.interfaces {
        let endpoint_id = member_to_endpoint.get(&interface.device_id).ok_or_else(|| {
            UnderlayError::InvalidIntent(format!(
                "interface references unknown switch member {}",
                interface.device_id.0
            ))
        })?;
        let state = states.get_mut(endpoint_id).ok_or_else(|| {
            UnderlayError::InvalidIntent(format!(
                "member references unknown management endpoint {}",
                endpoint_id.0
            ))
        })?;
        state.interfaces.insert(
            interface.name.clone(),
            InterfaceConfig {
                name: interface.name.clone(),
                admin_state: interface.admin_state.clone(),
                description: interface.description.clone(),
                mode: interface.mode.clone(),
            },
        );
    }

    for binding in &intent.acl_bindings {
        let endpoint_id = member_to_endpoint.get(&binding.device_id).ok_or_else(|| {
            UnderlayError::InvalidIntent(format!(
                "ACL binding references unknown switch member {}",
                binding.device_id.0
            ))
        })?;
        let state = states.get_mut(endpoint_id).ok_or_else(|| {
            UnderlayError::InvalidIntent(format!(
                "member references unknown management endpoint {}",
                endpoint_id.0
            ))
        })?;
        let binding = AclBinding {
            interface_name: binding.interface_name.clone(),
            direction: binding.direction.clone(),
            acl_id: binding.acl_id,
        };
        state.acl_bindings.insert(binding.key(), binding);
    }

    for binding in &intent.delete_acl_bindings {
        let endpoint_id = member_to_endpoint.get(&binding.device_id).ok_or_else(|| {
            UnderlayError::InvalidIntent(format!(
                "ACL binding delete references unknown switch member {}",
                binding.device_id.0
            ))
        })?;
        let state = states.get_mut(endpoint_id).ok_or_else(|| {
            UnderlayError::InvalidIntent(format!(
                "member references unknown management endpoint {}",
                endpoint_id.0
            ))
        })?;
        let binding = AclBinding {
            interface_name: binding.interface_name.clone(),
            direction: binding.direction.clone(),
            acl_id: binding.acl_id,
        };
        state.delete_acl_bindings.insert(binding.key(), binding);
    }

    Ok(states.into_values().collect())
}
