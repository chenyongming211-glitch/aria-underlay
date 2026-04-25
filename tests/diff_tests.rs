use aria_underlay::engine::diff::{compute_diff, ChangeOp, ChangeSet};
use aria_underlay::model::{AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::state::DeviceShadowState;

#[test]
fn empty_change_set_is_noop() {
    let change_set = ChangeSet::empty(DeviceId("leaf-a".into()));
    assert!(change_set.is_empty());
}

#[test]
fn identical_desired_and_current_is_noop() {
    let desired = desired_state(vec![vlan(100, Some("prod"), None)], vec![access_interface(
        "GE1/0/1",
        Some("server"),
        100,
    )]);
    let current = shadow_state(vec![vlan(100, Some("prod"), None)], vec![access_interface(
        "GE1/0/1",
        Some("server"),
        100,
    )]);

    let change_set = compute_diff(&desired, &current);

    assert!(change_set.is_empty());
}

#[test]
fn missing_vlan_creates_vlan() {
    let desired = desired_state(vec![vlan(100, Some("prod"), None)], vec![]);
    let current = shadow_state(vec![], vec![]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![ChangeOp::CreateVlan(vlan(100, Some("prod"), None))]
    );
}

#[test]
fn changed_vlan_updates_vlan() {
    let desired = desired_state(vec![vlan(100, Some("prod"), None)], vec![]);
    let current = shadow_state(vec![vlan(100, Some("old"), None)], vec![]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![ChangeOp::UpdateVlan {
            before: vlan(100, Some("old"), None),
            after: vlan(100, Some("prod"), None),
        }]
    );
}

#[test]
fn extra_vlan_deletes_vlan() {
    let desired = desired_state(vec![], vec![]);
    let current = shadow_state(vec![vlan(200, Some("old"), None)], vec![]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(change_set.ops, vec![ChangeOp::DeleteVlan { vlan_id: 200 }]);
}

#[test]
fn trunk_vlan_order_and_duplicates_are_noop() {
    let desired = desired_state(vec![], vec![trunk_interface(
        "GE1/0/1",
        None,
        Some(100),
        vec![300, 100, 200],
    )]);
    let current = shadow_state(vec![], vec![trunk_interface(
        "GE1/0/1",
        None,
        Some(100),
        vec![100, 200, 300, 300],
    )]);

    let change_set = compute_diff(&desired, &current);

    assert!(change_set.is_empty());
}

#[test]
fn empty_description_and_none_are_noop() {
    let desired = desired_state(vec![vlan(100, Some("prod"), None)], vec![access_interface(
        "GE1/0/1",
        None,
        100,
    )]);
    let current = shadow_state(vec![vlan(100, Some("prod"), Some(""))], vec![access_interface(
        "GE1/0/1",
        Some(""),
        100,
    )]);

    let change_set = compute_diff(&desired, &current);

    assert!(change_set.is_empty());
}

#[test]
fn canonical_interface_name_prevents_alias_diff() {
    let desired = desired_state(vec![], vec![access_interface("GE1/0/1", None, 100)]);
    let current = shadow_state(
        vec![],
        vec![access_interface("GigabitEthernet1/0/1", None, 100)],
    );

    let change_set = compute_diff(&desired, &current);

    assert!(change_set.is_empty());
}

fn desired_state(vlans: Vec<VlanConfig>, interfaces: Vec<InterfaceConfig>) -> DeviceDesiredState {
    DeviceDesiredState {
        device_id: DeviceId("leaf-a".into()),
        vlans: vlans.into_iter().map(|vlan| (vlan.vlan_id, vlan)).collect(),
        interfaces: interfaces
            .into_iter()
            .map(|interface| (interface.name.clone(), interface))
            .collect(),
    }
}

fn shadow_state(vlans: Vec<VlanConfig>, interfaces: Vec<InterfaceConfig>) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId("leaf-a".into()),
        revision: 1,
        vlans: vlans.into_iter().map(|vlan| (vlan.vlan_id, vlan)).collect(),
        interfaces: interfaces
            .into_iter()
            .map(|interface| (interface.name.clone(), interface))
            .collect(),
        warnings: vec![],
    }
}

fn vlan(vlan_id: u16, name: Option<&str>, description: Option<&str>) -> VlanConfig {
    VlanConfig {
        vlan_id,
        name: name.map(str::to_string),
        description: description.map(str::to_string),
    }
}

fn access_interface(name: &str, description: Option<&str>, vlan_id: u16) -> InterfaceConfig {
    InterfaceConfig {
        name: name.into(),
        admin_state: AdminState::Up,
        description: description.map(str::to_string),
        mode: PortMode::Access { vlan_id },
    }
}

fn trunk_interface(
    name: &str,
    description: Option<&str>,
    native_vlan: Option<u16>,
    allowed_vlans: Vec<u16>,
) -> InterfaceConfig {
    InterfaceConfig {
        name: name.into(),
        admin_state: AdminState::Up,
        description: description.map(str::to_string),
        mode: PortMode::Trunk {
            native_vlan,
            allowed_vlans,
        },
    }
}
