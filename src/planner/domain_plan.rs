use std::collections::BTreeMap;

use crate::intent::validation::validate_underlay_domain_intent;
use crate::intent::UnderlayDomainIntent;
use crate::model::{DeviceId, InterfaceConfig, VlanConfig};
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

    Ok(states.into_values().collect())
}
