use std::collections::{BTreeMap, BTreeSet};

use crate::intent::UnderlayDomainIntent;
use crate::model::{DeviceId, InterfaceConfig, VlanConfig};
use crate::planner::device_plan::DeviceDesiredState;
use crate::{UnderlayError, UnderlayResult};

pub fn plan_underlay_domain(
    intent: &UnderlayDomainIntent,
) -> UnderlayResult<Vec<DeviceDesiredState>> {
    validate_domain_shape(intent)?;

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

fn validate_domain_shape(intent: &UnderlayDomainIntent) -> UnderlayResult<()> {
    if intent.endpoints.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "underlay domain has no management endpoints".into(),
        ));
    }
    if intent.members.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "underlay domain has no switch members".into(),
        ));
    }

    let endpoint_ids = intent
        .endpoints
        .iter()
        .map(|endpoint| endpoint.endpoint_id.as_str())
        .collect::<BTreeSet<_>>();

    for member in &intent.members {
        if !endpoint_ids.contains(member.management_endpoint_id.as_str()) {
            return Err(UnderlayError::InvalidIntent(format!(
                "switch member {} references unknown management endpoint {}",
                member.member_id, member.management_endpoint_id
            )));
        }
    }

    Ok(())
}
