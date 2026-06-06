use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    bgp_neighbor_key, AclBinding, AclConfig, AclRule, BgpNeighbor, BgpProcess, InterfaceConfig,
    PortMode, VlanConfig,
};
use crate::planner::device_plan::DeviceDesiredState;
use crate::state::DeviceShadowState;

pub trait Normalize {
    fn normalize(self) -> Self;
}

impl Normalize for VlanConfig {
    fn normalize(mut self) -> Self {
        if self.name.as_deref() == Some("") {
            self.name = None;
        }
        if self.description.as_deref() == Some("") {
            self.description = None;
        }
        self
    }
}

impl Normalize for InterfaceConfig {
    fn normalize(mut self) -> Self {
        self.name = canonical_interface_name(&self.name);

        if self.description.as_deref() == Some("") {
            self.description = None;
        }

        if let PortMode::Trunk { allowed_vlans, .. } = &mut self.mode {
            allowed_vlans.sort_unstable();
            allowed_vlans.dedup();
        }

        self
    }
}

pub fn canonical_interface_name(name: &str) -> String {
    let trimmed = name.trim();
    for (long_name, short_name) in [
        ("GigabitEthernet", "GE"),
        ("Ten-GigabitEthernet", "XGE"),
        ("FortyGigE", "FGE"),
    ] {
        if let Some(rest) = trimmed.strip_prefix(long_name) {
            return format!("{short_name}{rest}");
        }
    }
    trimmed.to_string()
}

impl Normalize for AclConfig {
    fn normalize(mut self) -> Self {
        if self.name.as_deref() == Some("") {
            self.name = None;
        }
        if self.description.as_deref() == Some("") {
            self.description = None;
        }
        self.rules = self
            .rules
            .into_iter()
            .map(Normalize::normalize)
            .collect::<Vec<_>>();
        self.rules.sort_by_key(|rule| rule.sequence);
        self
    }
}

impl Normalize for AclRule {
    fn normalize(mut self) -> Self {
        if self.description.as_deref() == Some("") {
            self.description = None;
        }
        self
    }
}

impl Normalize for AclBinding {
    fn normalize(mut self) -> Self {
        self.interface_name = canonical_interface_name(&self.interface_name);
        self
    }
}

impl Normalize for BgpProcess {
    fn normalize(mut self) -> Self {
        self.vrf = self.vrf.trim().to_string();
        if self.router_id.as_deref() == Some("") {
            self.router_id = None;
        }
        self
    }
}

impl Normalize for BgpNeighbor {
    fn normalize(mut self) -> Self {
        self.vrf = self.vrf.trim().to_string();
        self.address = self.address.trim().to_string();
        if self.description.as_deref() == Some("") {
            self.description = None;
        }
        if self.import_policy.as_deref() == Some("") {
            self.import_policy = None;
        }
        if self.export_policy.as_deref() == Some("") {
            self.export_policy = None;
        }
        self
    }
}

pub fn normalize_desired_state(mut state: DeviceDesiredState) -> DeviceDesiredState {
    state.vlans = state
        .vlans
        .into_values()
        .map(|vlan| {
            let vlan = vlan.normalize();
            (vlan.vlan_id, vlan)
        })
        .collect::<BTreeMap<_, _>>();

    state.interfaces = state
        .interfaces
        .into_values()
        .map(|interface| {
            let interface = interface.normalize();
            (interface.name.clone(), interface)
        })
        .collect::<BTreeMap<_, _>>();

    state.delete_interface_names = state
        .delete_interface_names
        .into_iter()
        .map(|name| canonical_interface_name(&name))
        .collect::<BTreeSet<_>>();

    state.acls = state
        .acls
        .into_values()
        .map(|acl| {
            let acl = acl.normalize();
            (acl.acl_id, acl)
        })
        .collect::<BTreeMap<_, _>>();

    state.acl_bindings = state
        .acl_bindings
        .into_values()
        .map(|binding| {
            let binding = binding.normalize();
            (binding.key(), binding)
        })
        .collect::<BTreeMap<_, _>>();

    state.delete_acl_bindings = state
        .delete_acl_bindings
        .into_values()
        .map(|binding| {
            let binding = binding.normalize();
            (binding.key(), binding)
        })
        .collect::<BTreeMap<_, _>>();

    state.bgp_processes = state
        .bgp_processes
        .into_values()
        .map(|process| {
            let process = process.normalize();
            (process.vrf.clone(), process)
        })
        .collect::<BTreeMap<_, _>>();

    state.bgp_neighbors = state
        .bgp_neighbors
        .into_values()
        .map(|neighbor| {
            let neighbor = neighbor.normalize();
            (neighbor.key(), neighbor)
        })
        .collect::<BTreeMap<_, _>>();

    state.delete_bgp_process_vrfs = state
        .delete_bgp_process_vrfs
        .into_iter()
        .map(|vrf| vrf.trim().to_string())
        .collect::<BTreeSet<_>>();

    state.delete_bgp_neighbors = state
        .delete_bgp_neighbors
        .into_values()
        .map(|neighbor| {
            let neighbor = neighbor.normalize();
            (bgp_neighbor_key(&neighbor.vrf, &neighbor.address), neighbor)
        })
        .collect::<BTreeMap<_, _>>();

    state
}

pub fn normalize_shadow_state(mut state: DeviceShadowState) -> DeviceShadowState {
    state.vlans = state
        .vlans
        .into_values()
        .map(|vlan| {
            let vlan = vlan.normalize();
            (vlan.vlan_id, vlan)
        })
        .collect::<BTreeMap<_, _>>();

    state.interfaces = state
        .interfaces
        .into_values()
        .map(|interface| {
            let interface = interface.normalize();
            (interface.name.clone(), interface)
        })
        .collect::<BTreeMap<_, _>>();

    state.acls = state
        .acls
        .into_values()
        .map(|acl| {
            let acl = acl.normalize();
            (acl.acl_id, acl)
        })
        .collect::<BTreeMap<_, _>>();

    state.acl_bindings = state
        .acl_bindings
        .into_values()
        .map(|binding| {
            let binding = binding.normalize();
            (binding.key(), binding)
        })
        .collect::<BTreeMap<_, _>>();

    state.bgp_processes = state
        .bgp_processes
        .into_values()
        .map(|process| {
            let process = process.normalize();
            (process.vrf.clone(), process)
        })
        .collect::<BTreeMap<_, _>>();

    state.bgp_neighbors = state
        .bgp_neighbors
        .into_values()
        .map(|neighbor| {
            let neighbor = neighbor.normalize();
            (neighbor.key(), neighbor)
        })
        .collect::<BTreeMap<_, _>>();

    state
}
