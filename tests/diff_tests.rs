use aria_underlay::engine::diff::{compute_diff, compute_merge_upsert_diff, ChangeOp, ChangeSet};
use aria_underlay::model::{
    AclAction, AclBinding, AclConfig, AclDirection, AclEndpoint, AclKind, AclProtocol, AclRule,
    AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig,
};
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

#[test]
fn canonical_interface_name_prevents_h3c_ten_gigabit_alias_diff() {
    let desired = desired_state(vec![], vec![access_interface("XGE1/0/1", None, 100)]);
    let current = shadow_state(
        vec![],
        vec![access_interface("Ten-GigabitEthernet1/0/1", None, 100)],
    );

    let change_set = compute_diff(&desired, &current);

    assert!(change_set.is_empty());
}

#[test]
fn missing_acl_creates_acl() {
    let desired = desired_state_with_acls(vec![], vec![], vec![acl(3999, "temporary")]);
    let current = shadow_state_with_acls(vec![], vec![], vec![]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![ChangeOp::CreateAcl(acl(3999, "temporary"))]
    );
}

#[test]
fn changed_acl_updates_acl() {
    let desired = desired_state_with_acls(vec![], vec![], vec![acl(3999, "new")]);
    let current = shadow_state_with_acls(vec![], vec![], vec![acl(3999, "old")]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![ChangeOp::UpdateAcl {
            before: acl(3999, "old"),
            after: acl(3999, "new"),
        }]
    );
}

#[test]
fn extra_acl_deletes_acl() {
    let desired = desired_state_with_acls(vec![], vec![], vec![]);
    let current = shadow_state_with_acls(vec![], vec![], vec![acl(3999, "old")]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(change_set.ops, vec![ChangeOp::DeleteAcl { acl_id: 3999 }]);
}

#[test]
fn missing_acl_binding_creates_binding() {
    let desired = desired_state_with_acl_bindings(
        vec![],
        vec![],
        vec![acl(3999, "temporary")],
        vec![acl_binding("GE1/0/13", AclDirection::Inbound, 3999)],
    );
    let current = shadow_state_with_acl_bindings(vec![], vec![], vec![], vec![]);

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![
            ChangeOp::CreateAcl(acl(3999, "temporary")),
            ChangeOp::CreateAclBinding(acl_binding(
                "GE1/0/13",
                AclDirection::Inbound,
                3999
            )),
        ]
    );
}

#[test]
fn changed_acl_binding_updates_binding() {
    let desired = desired_state_with_acl_bindings(
        vec![],
        vec![],
        vec![acl(3999, "new")],
        vec![acl_binding("GE1/0/13", AclDirection::Inbound, 3999)],
    );
    let current = shadow_state_with_acl_bindings(
        vec![],
        vec![],
        vec![acl(3998, "old")],
        vec![acl_binding("GE1/0/13", AclDirection::Inbound, 3998)],
    );

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![
            ChangeOp::CreateAcl(acl(3999, "new")),
            ChangeOp::UpdateAclBinding {
                before: acl_binding("GE1/0/13", AclDirection::Inbound, 3998),
                after: acl_binding("GE1/0/13", AclDirection::Inbound, 3999),
            },
            ChangeOp::DeleteAcl { acl_id: 3998 },
        ]
    );
}

#[test]
fn extra_acl_binding_deletes_binding() {
    let desired = desired_state_with_acl_bindings(vec![], vec![], vec![], vec![]);
    let current = shadow_state_with_acl_bindings(
        vec![],
        vec![],
        vec![acl(3999, "old")],
        vec![acl_binding("GE1/0/13", AclDirection::Outbound, 3999)],
    );

    let change_set = compute_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![
            ChangeOp::DeleteAclBinding {
                interface_name: "GE1/0/13".into(),
                direction: AclDirection::Outbound,
                acl_id: 3999,
            },
            ChangeOp::DeleteAcl { acl_id: 3999 },
        ]
    );
}

#[test]
fn merge_upsert_does_not_infer_deletes_from_absence() {
    let desired = desired_state_with_acl_bindings(vec![], vec![], vec![], vec![]);
    let current = shadow_state_with_acl_bindings(
        vec![vlan(200, Some("existing"), None)],
        vec![],
        vec![acl(3999, "existing")],
        vec![acl_binding("GE1/0/13", AclDirection::Inbound, 3999)],
    );

    let change_set = compute_merge_upsert_diff(&desired, &current);

    assert!(change_set.is_empty());
}

#[test]
fn merge_upsert_deletes_only_explicit_delete_targets() {
    let mut desired = desired_state_with_acl_bindings(vec![], vec![], vec![], vec![]);
    desired.delete_vlan_ids.insert(200);
    desired.delete_acl_ids.insert(3999);
    let delete_binding = acl_binding("GE1/0/13", AclDirection::Inbound, 3999);
    desired
        .delete_acl_bindings
        .insert(delete_binding.key(), delete_binding.clone());
    let current = shadow_state_with_acl_bindings(
        vec![vlan(200, Some("existing"), None), vlan(300, Some("kept"), None)],
        vec![],
        vec![acl(3999, "existing"), acl(3998, "kept")],
        vec![
            acl_binding("GE1/0/13", AclDirection::Inbound, 3999),
            acl_binding("GE1/0/14", AclDirection::Inbound, 3998),
        ],
    );

    let change_set = compute_merge_upsert_diff(&desired, &current);

    assert_eq!(
        change_set.ops,
        vec![
            ChangeOp::DeleteVlan { vlan_id: 200 },
            ChangeOp::DeleteAclBinding {
                interface_name: "GE1/0/13".into(),
                direction: AclDirection::Inbound,
                acl_id: 3999,
            },
            ChangeOp::DeleteAcl { acl_id: 3999 },
        ]
    );
}

#[test]
fn applying_change_set_updates_shadow_without_dropping_unrelated_state() {
    let mut desired = desired_state_with_acl_bindings(
        vec![vlan(300, Some("new"), None)],
        vec![],
        vec![],
        vec![],
    );
    desired.delete_vlan_ids.insert(200);
    let current = shadow_state_with_acl_bindings(
        vec![vlan(100, Some("kept"), None), vlan(200, Some("remove"), None)],
        vec![],
        vec![],
        vec![],
    );
    let change_set = compute_merge_upsert_diff(&desired, &current);

    let updated = change_set.apply_to_shadow(Some(&current), &desired, 0);

    assert!(updated.vlans.contains_key(&100));
    assert!(!updated.vlans.contains_key(&200));
    assert_eq!(updated.vlans[&300].name.as_deref(), Some("new"));
}

fn desired_state(vlans: Vec<VlanConfig>, interfaces: Vec<InterfaceConfig>) -> DeviceDesiredState {
    desired_state_with_acls(vlans, interfaces, vec![])
}

fn desired_state_with_acls(
    vlans: Vec<VlanConfig>,
    interfaces: Vec<InterfaceConfig>,
    acls: Vec<AclConfig>,
) -> DeviceDesiredState {
    desired_state_with_acl_bindings(vlans, interfaces, acls, vec![])
}

fn desired_state_with_acl_bindings(
    vlans: Vec<VlanConfig>,
    interfaces: Vec<InterfaceConfig>,
    acls: Vec<AclConfig>,
    acl_bindings: Vec<AclBinding>,
) -> DeviceDesiredState {
    DeviceDesiredState {
        device_id: DeviceId("leaf-a".into()),
        vlans: vlans.into_iter().map(|vlan| (vlan.vlan_id, vlan)).collect(),
        interfaces: interfaces
            .into_iter()
            .map(|interface| (interface.name.clone(), interface))
            .collect(),
        acls: acls.into_iter().map(|acl| (acl.acl_id, acl)).collect(),
        acl_bindings: acl_bindings
            .into_iter()
            .map(|binding| (binding.key(), binding))
            .collect(),
        delete_vlan_ids: Default::default(),
        delete_acl_ids: Default::default(),
        delete_acl_bindings: Default::default(),
    }
}

fn shadow_state(vlans: Vec<VlanConfig>, interfaces: Vec<InterfaceConfig>) -> DeviceShadowState {
    shadow_state_with_acls(vlans, interfaces, vec![])
}

fn shadow_state_with_acls(
    vlans: Vec<VlanConfig>,
    interfaces: Vec<InterfaceConfig>,
    acls: Vec<AclConfig>,
) -> DeviceShadowState {
    shadow_state_with_acl_bindings(vlans, interfaces, acls, vec![])
}

fn shadow_state_with_acl_bindings(
    vlans: Vec<VlanConfig>,
    interfaces: Vec<InterfaceConfig>,
    acls: Vec<AclConfig>,
    acl_bindings: Vec<AclBinding>,
) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId("leaf-a".into()),
        revision: 1,
        vlans: vlans.into_iter().map(|vlan| (vlan.vlan_id, vlan)).collect(),
        interfaces: interfaces
            .into_iter()
            .map(|interface| (interface.name.clone(), interface))
            .collect(),
        acls: acls.into_iter().map(|acl| (acl.acl_id, acl)).collect(),
        acl_bindings: acl_bindings
            .into_iter()
            .map(|binding| (binding.key(), binding))
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

fn acl(acl_id: u16, description: &str) -> AclConfig {
    AclConfig {
        acl_id,
        kind: AclKind::AdvancedIpv4,
        name: None,
        description: Some(description.into()),
        rules: vec![AclRule {
            sequence: 10,
            action: AclAction::Permit,
            protocol: AclProtocol::Ip,
            source: Some(AclEndpoint {
                address: "192.0.2.1".into(),
                wildcard: "0.0.0.0".into(),
            }),
            destination: None,
            source_port_eq: None,
            destination_port_eq: None,
            description: None,
        }],
    }
}

fn acl_binding(interface_name: &str, direction: AclDirection, acl_id: u16) -> AclBinding {
    AclBinding {
        interface_name: interface_name.into(),
        direction,
        acl_id,
    }
}
