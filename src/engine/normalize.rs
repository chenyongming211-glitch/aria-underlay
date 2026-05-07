use std::collections::BTreeMap;

use crate::model::{InterfaceConfig, PortMode, VlanConfig};
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

    state
}
