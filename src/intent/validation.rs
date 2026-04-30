use std::collections::BTreeSet;

use crate::intent::{SwitchPairIntent, UnderlayDomainIntent, UnderlayTopology};
use crate::model::{is_canonical_identifier, DeviceId, PortMode};
use crate::{UnderlayError, UnderlayResult};

pub fn validate_switch_pair_intent(intent: &SwitchPairIntent) -> UnderlayResult<()> {
    if intent.pair_id.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "switch pair intent has empty pair_id".into(),
        ));
    }
    if intent.switches.is_empty() {
        return Err(UnderlayError::InvalidIntent("intent has no switches".into()));
    }

    let mut switch_ids = BTreeSet::new();
    for switch in &intent.switches {
        validate_device_id("switch device_id", &switch.device_id)?;
        if !switch_ids.insert(switch.device_id.clone()) {
            return Err(UnderlayError::InvalidIntent(format!(
                "duplicate switch device_id {}",
                switch.device_id.0
            )));
        }
    }

    validate_vlans(
        intent.vlans.iter().map(|vlan| vlan.vlan_id),
        "switch pair",
    )?;
    let declared_vlans = intent
        .vlans
        .iter()
        .map(|vlan| vlan.vlan_id)
        .collect::<BTreeSet<_>>();
    validate_interfaces(
        intent
            .interfaces
            .iter()
            .map(|iface| (&iface.device_id, iface.name.as_str(), &iface.mode)),
        &switch_ids,
        &declared_vlans,
        "switch pair",
        "switch",
    )?;

    Ok(())
}

pub fn validate_underlay_domain_intent(intent: &UnderlayDomainIntent) -> UnderlayResult<()> {
    validate_non_empty("underlay domain_id", &intent.domain_id)?;
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

    validate_topology_shape(intent)?;

    let mut endpoint_ids = BTreeSet::new();
    for endpoint in &intent.endpoints {
        validate_identifier("management endpoint endpoint_id", &endpoint.endpoint_id)?;
        validate_non_empty("management endpoint host", &endpoint.host)?;
        validate_non_empty("management endpoint secret_ref", &endpoint.secret_ref)?;
        if endpoint.port == 0 {
            return Err(UnderlayError::InvalidIntent(format!(
                "management endpoint {} has invalid port 0",
                endpoint.endpoint_id
            )));
        }
        if !endpoint_ids.insert(endpoint.endpoint_id.as_str()) {
            return Err(UnderlayError::InvalidIntent(format!(
                "duplicate management endpoint {}",
                endpoint.endpoint_id
            )));
        }
    }

    let mut member_ids = BTreeSet::new();
    for member in &intent.members {
        validate_identifier("switch member member_id", &member.member_id)?;
        validate_identifier(
            "switch member management_endpoint_id",
            &member.management_endpoint_id,
        )?;
        if !member_ids.insert(DeviceId(member.member_id.clone())) {
            return Err(UnderlayError::InvalidIntent(format!(
                "duplicate switch member {}",
                member.member_id
            )));
        }
        if !endpoint_ids.contains(member.management_endpoint_id.as_str()) {
            return Err(UnderlayError::InvalidIntent(format!(
                "switch member {} references unknown management endpoint {}",
                member.member_id, member.management_endpoint_id
            )));
        }
    }

    validate_vlans(
        intent.vlans.iter().map(|vlan| vlan.vlan_id),
        "underlay domain",
    )?;
    let declared_vlans = intent
        .vlans
        .iter()
        .map(|vlan| vlan.vlan_id)
        .collect::<BTreeSet<_>>();
    validate_interfaces(
        intent
            .interfaces
            .iter()
            .map(|iface| (&iface.device_id, iface.name.as_str(), &iface.mode)),
        &member_ids,
        &declared_vlans,
        "underlay domain",
        "switch member",
    )?;

    Ok(())
}

fn validate_topology_shape(intent: &UnderlayDomainIntent) -> UnderlayResult<()> {
    match intent.topology {
        UnderlayTopology::StackSingleManagementIp if intent.endpoints.len() != 1 => {
            Err(UnderlayError::InvalidIntent(format!(
                "stack topology requires exactly one management endpoint, got {}",
                intent.endpoints.len()
            )))
        }
        UnderlayTopology::MlagDualManagementIp if intent.endpoints.len() != 2 => {
            Err(UnderlayError::InvalidIntent(format!(
                "mlag topology requires exactly two management endpoints, got {}",
                intent.endpoints.len()
            )))
        }
        _ => Ok(()),
    }
}

fn validate_non_empty(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!("{field} is empty")));
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str) -> UnderlayResult<()> {
    if !is_canonical_identifier(value) {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} is invalid: {}",
            DeviceId::canonical_rule()
        )));
    }
    Ok(())
}

fn validate_device_id(field: &str, device_id: &DeviceId) -> UnderlayResult<()> {
    if !device_id.is_canonical() {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} {} is invalid: {}",
            device_id.0,
            DeviceId::canonical_rule()
        )));
    }
    Ok(())
}

fn validate_vlans<I>(vlan_ids: I, context: &str) -> UnderlayResult<()>
where
    I: IntoIterator<Item = u16>,
{
    let mut seen = BTreeSet::new();
    for vlan_id in vlan_ids {
        if !(1..=4094).contains(&vlan_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has invalid vlan_id {vlan_id}; valid range is 1..=4094"
            )));
        }
        if !seen.insert(vlan_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has duplicate vlan_id {vlan_id}"
            )));
        }
    }
    Ok(())
}

fn validate_interfaces<'a, I>(
    interfaces: I,
    valid_device_ids: &BTreeSet<DeviceId>,
    declared_vlans: &BTreeSet<u16>,
    context: &str,
    unknown_device_label: &str,
) -> UnderlayResult<()>
where
    I: IntoIterator<Item = (&'a DeviceId, &'a str, &'a PortMode)>,
{
    let mut seen = BTreeSet::new();
    for (device_id, name, mode) in interfaces {
        validate_non_empty("interface name", name)?;
        if !valid_device_ids.contains(device_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} interface {} references unknown {} {}",
                name, unknown_device_label, device_id.0
            )));
        }
        if !seen.insert((device_id.clone(), name.to_string())) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has duplicate interface {} on {}",
                name, device_id.0
            )));
        }
        validate_port_mode(mode, declared_vlans, context, device_id, name)?;
    }
    Ok(())
}

fn validate_port_mode(
    mode: &PortMode,
    declared_vlans: &BTreeSet<u16>,
    context: &str,
    device_id: &DeviceId,
    interface_name: &str,
) -> UnderlayResult<()> {
    match mode {
        PortMode::Access { vlan_id } => validate_declared_vlan(
            *vlan_id,
            declared_vlans,
            context,
            device_id,
            interface_name,
        ),
        PortMode::Trunk {
            native_vlan,
            allowed_vlans,
        } => {
            if allowed_vlans.is_empty() && native_vlan.is_none() {
                return Err(UnderlayError::InvalidIntent(format!(
                    "{context} trunk interface {} on {} has no native or allowed VLAN",
                    interface_name, device_id.0
                )));
            }

            let mut seen_allowed = BTreeSet::new();
            if let Some(vlan_id) = native_vlan {
                validate_declared_vlan(
                    *vlan_id,
                    declared_vlans,
                    context,
                    device_id,
                    interface_name,
                )?;
            }
            for vlan_id in allowed_vlans {
                validate_declared_vlan(
                    *vlan_id,
                    declared_vlans,
                    context,
                    device_id,
                    interface_name,
                )?;
                if !seen_allowed.insert(*vlan_id) {
                    return Err(UnderlayError::InvalidIntent(format!(
                        "{context} trunk interface {} on {} has duplicate allowed VLAN {}",
                        interface_name, device_id.0, vlan_id
                    )));
                }
            }
            Ok(())
        }
    }
}

fn validate_declared_vlan(
    vlan_id: u16,
    declared_vlans: &BTreeSet<u16>,
    context: &str,
    device_id: &DeviceId,
    interface_name: &str,
) -> UnderlayResult<()> {
    if !declared_vlans.contains(&vlan_id) {
        return Err(UnderlayError::InvalidIntent(format!(
            "{context} interface {} on {} references undeclared VLAN {}",
            interface_name, device_id.0, vlan_id
        )));
    }
    Ok(())
}
