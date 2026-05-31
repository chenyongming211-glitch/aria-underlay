use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotSource {
    pub system: String,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotDevice {
    pub device_id: String,
    pub vendor: String,
    pub model: String,
    pub os_version: String,
    pub model_profile_ref: Option<String>,
    pub source: SotSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotInterface {
    pub device_id: String,
    pub name: String,
    pub description: Option<String>,
    pub source: SotSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotVlan {
    pub device_id: String,
    pub vlan_id: u16,
    pub name: Option<String>,
    pub owner: String,
    pub source: SotSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotAcl {
    pub device_id: String,
    pub acl_id: u16,
    pub owner: String,
    pub source: SotSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotPolicyIntent {
    pub device_id: String,
    pub policy_id: String,
    pub owner: String,
    pub source: SotSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SotBgpNeighbor {
    pub device_id: String,
    pub vrf: String,
    pub neighbor_address: String,
    pub remote_as: u32,
    pub owner: String,
    pub source: SotSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SotSnapshot {
    pub devices: Vec<SotDevice>,
    pub interfaces: Vec<SotInterface>,
    pub vlans: Vec<SotVlan>,
    pub acls: Vec<SotAcl>,
    pub policy_intents: Vec<SotPolicyIntent>,
    pub bgp_neighbors: Vec<SotBgpNeighbor>,
}

impl SotSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        let device_ids = self.validate_devices()?;
        self.validate_interfaces(&device_ids)?;
        self.validate_vlans(&device_ids)?;
        self.validate_acls(&device_ids)?;
        self.validate_policy_intents(&device_ids)?;
        self.validate_bgp_neighbors(&device_ids)?;
        Ok(())
    }

    fn validate_devices(&self) -> Result<BTreeSet<&str>, String> {
        let mut device_ids = BTreeSet::new();
        for device in &self.devices {
            let device_id = require_non_empty("SoT device_id", &device.device_id)?;
            validate_source(&device.source, format!("SoT device {device_id} source"))?;
            if !device_ids.insert(device_id) {
                return Err(format!("duplicate SoT device_id {device_id}"));
            }
        }
        Ok(device_ids)
    }

    fn validate_interfaces(&self, device_ids: &BTreeSet<&str>) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for interface in &self.interfaces {
            ensure_known_device(
                device_ids,
                &interface.device_id,
                format!("SoT interface {}", interface.name),
            )?;
            let name = require_non_empty("SoT interface name", &interface.name)?;
            validate_source(
                &interface.source,
                format!("SoT interface {}/{} source", interface.device_id, name),
            )?;
            let key = (interface.device_id.as_str(), name);
            if !seen.insert(key) {
                return Err(format!(
                    "duplicate SoT interface {}/{}",
                    interface.device_id, name
                ));
            }
        }
        Ok(())
    }

    fn validate_vlans(&self, device_ids: &BTreeSet<&str>) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for vlan in &self.vlans {
            ensure_known_device(
                device_ids,
                &vlan.device_id,
                format!("SoT vlan {}", vlan.vlan_id),
            )?;
            require_non_empty(format!("SoT vlan {} owner", vlan.vlan_id), &vlan.owner)?;
            validate_source(
                &vlan.source,
                format!("SoT vlan {}/{} source", vlan.device_id, vlan.vlan_id),
            )?;
            let key = (vlan.device_id.as_str(), vlan.vlan_id);
            if !seen.insert(key) {
                return Err(format!("duplicate SoT vlan {}/{}", vlan.device_id, vlan.vlan_id));
            }
        }
        Ok(())
    }

    fn validate_acls(&self, device_ids: &BTreeSet<&str>) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for acl in &self.acls {
            ensure_known_device(
                device_ids,
                &acl.device_id,
                format!("SoT acl {}", acl.acl_id),
            )?;
            require_non_empty(format!("SoT acl {} owner", acl.acl_id), &acl.owner)?;
            validate_source(
                &acl.source,
                format!("SoT acl {}/{} source", acl.device_id, acl.acl_id),
            )?;
            let key = (acl.device_id.as_str(), acl.acl_id);
            if !seen.insert(key) {
                return Err(format!("duplicate SoT acl {}/{}", acl.device_id, acl.acl_id));
            }
        }
        Ok(())
    }

    fn validate_policy_intents(&self, device_ids: &BTreeSet<&str>) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for policy in &self.policy_intents {
            ensure_known_device(
                device_ids,
                &policy.device_id,
                format!("SoT policy {}", policy.policy_id),
            )?;
            let policy_id = require_non_empty("SoT policy_id", &policy.policy_id)?;
            require_non_empty(format!("SoT policy {policy_id} owner"), &policy.owner)?;
            validate_source(
                &policy.source,
                format!("SoT policy {}/{} source", policy.device_id, policy_id),
            )?;
            let key = (policy.device_id.as_str(), policy_id);
            if !seen.insert(key) {
                return Err(format!(
                    "duplicate SoT policy {}/{}",
                    policy.device_id, policy_id
                ));
            }
        }
        Ok(())
    }

    fn validate_bgp_neighbors(&self, device_ids: &BTreeSet<&str>) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for neighbor in &self.bgp_neighbors {
            ensure_known_device(
                device_ids,
                &neighbor.device_id,
                format!("SoT bgp neighbor {}", neighbor.neighbor_address),
            )?;
            let vrf = require_non_empty("SoT bgp vrf", &neighbor.vrf)?;
            let address = require_non_empty(
                "SoT bgp neighbor_address",
                &neighbor.neighbor_address,
            )?;
            require_non_empty(
                format!("SoT bgp neighbor {address} owner"),
                &neighbor.owner,
            )?;
            validate_source(
                &neighbor.source,
                format!(
                    "SoT bgp neighbor {}/{}/{} source",
                    neighbor.device_id, vrf, address
                ),
            )?;
            let key = (neighbor.device_id.as_str(), vrf, address);
            if !seen.insert(key) {
                return Err(format!(
                    "duplicate SoT bgp neighbor {}/{}/{}",
                    neighbor.device_id, vrf, address
                ));
            }
        }
        Ok(())
    }
}

fn ensure_known_device(
    device_ids: &BTreeSet<&str>,
    device_id: &str,
    subject: String,
) -> Result<(), String> {
    if device_ids.contains(device_id) {
        return Ok(());
    }
    Err(format!("{subject} references unknown device_id {device_id}"))
}

fn require_non_empty<'a>(field: impl AsRef<str>, value: &'a str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{} must not be empty", field.as_ref()));
    }
    Ok(trimmed)
}

fn validate_source(source: &SotSource, field: impl AsRef<str>) -> Result<(), String> {
    let field = field.as_ref();
    require_non_empty(format!("{field} system"), &source.system)?;
    require_non_empty(format!("{field} reference"), &source.reference)?;
    Ok(())
}
