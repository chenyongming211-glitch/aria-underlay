use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use crate::intent::{
    acl::AclBindingIntent, SwitchPairIntent, UnderlayDomainIntent, UnderlayTopology,
};
use crate::model::{
    acl_binding_key, is_canonical_identifier, AclConfig, AclProtocol, DeviceId, PortMode,
};
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
    validate_vlans(
        intent.delete_vlan_ids.iter().copied(),
        "underlay domain delete",
    )?;
    let declared_vlans = intent
        .vlans
        .iter()
        .map(|vlan| vlan.vlan_id)
        .collect::<BTreeSet<_>>();
    for vlan_id in &intent.delete_vlan_ids {
        if declared_vlans.contains(vlan_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "underlay domain cannot upsert and delete VLAN {vlan_id} in the same request"
            )));
        }
    }
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
    validate_acls(
        intent.acls.iter().map(|acl| AclConfig {
            acl_id: acl.acl_id,
            name: acl.name.clone(),
            description: acl.description.clone(),
            rules: acl.rules.clone(),
        }),
        "underlay domain",
    )?;
    validate_acl_ids(
        intent.delete_acl_ids.iter().copied(),
        "underlay domain delete",
    )?;
    let declared_acls = intent
        .acls
        .iter()
        .map(|acl| acl.acl_id)
        .collect::<BTreeSet<_>>();
    for acl_id in &intent.delete_acl_ids {
        if declared_acls.contains(acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "underlay domain cannot upsert and delete ACL {acl_id} in the same request"
            )));
        }
    }
    validate_acl_bindings(
        &intent.acl_bindings,
        &member_ids,
        &declared_acls,
        "underlay domain",
    )?;
    validate_acl_binding_deletes(
        &intent.delete_acl_bindings,
        &intent.acl_bindings,
        &member_ids,
        "underlay domain",
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
        UnderlayTopology::SmallFabric if intent.endpoints.len() < 2 => {
            Err(UnderlayError::InvalidIntent(format!(
                "small fabric topology requires at least two management endpoints, got {}",
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

fn validate_acls<I>(acls: I, context: &str) -> UnderlayResult<()>
where
    I: IntoIterator<Item = AclConfig>,
{
    let mut seen = BTreeSet::new();
    for acl in acls {
        if !(3000..=3999).contains(&acl.acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has invalid IPv4 advanced acl_id {}; valid range is 3000..=3999",
                acl.acl_id
            )));
        }
        if !seen.insert(acl.acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has duplicate acl_id {}",
                acl.acl_id
            )));
        }

        let mut rule_ids = BTreeSet::new();
        for rule in &acl.rules {
            if !rule_ids.insert(rule.sequence) {
                return Err(UnderlayError::InvalidIntent(format!(
                    "{context} ACL {} has duplicate rule sequence {}",
                    acl.acl_id, rule.sequence
                )));
            }
            if rule.source_port_eq.is_some()
                && !matches!(rule.protocol, AclProtocol::Tcp | AclProtocol::Udp)
            {
                return Err(UnderlayError::InvalidIntent(format!(
                    "{context} ACL {} rule {} has source_port_eq but protocol is not tcp/udp",
                    acl.acl_id, rule.sequence
                )));
            }
            if rule.destination_port_eq.is_some()
                && !matches!(rule.protocol, AclProtocol::Tcp | AclProtocol::Udp)
            {
                return Err(UnderlayError::InvalidIntent(format!(
                    "{context} ACL {} rule {} has destination_port_eq but protocol is not tcp/udp",
                    acl.acl_id, rule.sequence
                )));
            }
            if matches!(rule.source_port_eq, Some(0)) || matches!(rule.destination_port_eq, Some(0))
            {
                return Err(UnderlayError::InvalidIntent(format!(
                    "{context} ACL {} rule {} has invalid port 0",
                    acl.acl_id, rule.sequence
                )));
            }
            if let Some(source) = &rule.source {
                validate_acl_endpoint(context, acl.acl_id, rule.sequence, "source", source)?;
            }
            if let Some(destination) = &rule.destination {
                validate_acl_endpoint(
                    context,
                    acl.acl_id,
                    rule.sequence,
                    "destination",
                    destination,
                )?;
            }
        }
    }
    Ok(())
}

fn validate_acl_ids<I>(acl_ids: I, context: &str) -> UnderlayResult<()>
where
    I: IntoIterator<Item = u16>,
{
    let mut seen = BTreeSet::new();
    for acl_id in acl_ids {
        if !(3000..=3999).contains(&acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has invalid IPv4 advanced acl_id {acl_id}; valid range is 3000..=3999"
            )));
        }
        if !seen.insert(acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has duplicate acl_id {acl_id}"
            )));
        }
    }
    Ok(())
}

fn validate_acl_endpoint(
    context: &str,
    acl_id: u16,
    sequence: u16,
    field: &str,
    endpoint: &crate::model::AclEndpoint,
) -> UnderlayResult<()> {
    endpoint.address.parse::<Ipv4Addr>().map_err(|_| {
        UnderlayError::InvalidIntent(format!(
            "{context} ACL {acl_id} rule {sequence} has invalid {field} IPv4 address {}",
            endpoint.address
        ))
    })?;
    endpoint.wildcard.parse::<Ipv4Addr>().map_err(|_| {
        UnderlayError::InvalidIntent(format!(
            "{context} ACL {acl_id} rule {sequence} has invalid {field} wildcard {}",
            endpoint.wildcard
        ))
    })?;
    Ok(())
}

fn validate_acl_bindings(
    bindings: &[AclBindingIntent],
    valid_device_ids: &BTreeSet<DeviceId>,
    declared_acls: &BTreeSet<u16>,
    context: &str,
) -> UnderlayResult<()> {
    let mut seen = BTreeSet::new();
    for binding in bindings {
        validate_non_empty("ACL binding interface_name", &binding.interface_name)?;
        if !valid_device_ids.contains(&binding.device_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} ACL binding for interface {} references unknown switch member {}",
                binding.interface_name, binding.device_id.0
            )));
        }
        if !declared_acls.contains(&binding.acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} ACL binding on {} references undeclared ACL {}",
                binding.interface_name, binding.acl_id
            )));
        }
        let key = (
            binding.device_id.clone(),
            acl_binding_key(&binding.interface_name, &binding.direction),
        );
        if !seen.insert(key) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has duplicate ACL binding on {} direction {:?}",
                binding.interface_name, binding.direction
            )));
        }
    }
    Ok(())
}

fn validate_acl_binding_deletes(
    deletes: &[AclBindingIntent],
    upserts: &[AclBindingIntent],
    valid_device_ids: &BTreeSet<DeviceId>,
    context: &str,
) -> UnderlayResult<()> {
    let mut upsert_keys = BTreeSet::new();
    for binding in upserts {
        upsert_keys.insert((
            binding.device_id.clone(),
            acl_binding_key(&binding.interface_name, &binding.direction),
        ));
    }

    let mut seen = BTreeSet::new();
    for binding in deletes {
        validate_non_empty("ACL binding delete interface_name", &binding.interface_name)?;
        if !valid_device_ids.contains(&binding.device_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} ACL binding delete for interface {} references unknown switch member {}",
                binding.interface_name, binding.device_id.0
            )));
        }
        if !(3000..=3999).contains(&binding.acl_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} ACL binding delete on {} has invalid IPv4 advanced acl_id {}; valid range is 3000..=3999",
                binding.interface_name, binding.acl_id
            )));
        }

        let key = (
            binding.device_id.clone(),
            acl_binding_key(&binding.interface_name, &binding.direction),
        );
        if upsert_keys.contains(&key) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} cannot upsert and delete ACL binding on {} direction {:?} in the same request",
                binding.interface_name, binding.direction
            )));
        }
        if !seen.insert(key) {
            return Err(UnderlayError::InvalidIntent(format!(
                "{context} has duplicate ACL binding delete on {} direction {:?}",
                binding.interface_name, binding.direction
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
