use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::validation::{
    validate_switch_pair_intent, validate_underlay_domain_intent,
};
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    AclBindingIntent, AclIntent, ManagementEndpointIntent, SwitchIntent, SwitchMemberIntent,
    SwitchPairIntent, UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{
    AclAction, AclDirection, AclEndpoint, AclProtocol, AclRule, AdminState, DeviceId, DeviceRole,
    PortMode, Vendor,
};

#[test]
fn switch_pair_rejects_invalid_vlan_range() {
    let mut intent = switch_pair_intent();
    intent.vlans[0].vlan_id = 0;

    let err = validate_switch_pair_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("invalid vlan_id 0"));
}

#[test]
fn switch_pair_rejects_interface_referencing_unknown_switch() {
    let mut intent = switch_pair_intent();
    intent.interfaces[0].device_id = DeviceId("missing-leaf".into());

    let err = validate_switch_pair_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("unknown switch missing-leaf"));
}

#[test]
fn switch_pair_rejects_access_vlan_not_declared() {
    let mut intent = switch_pair_intent();
    intent.interfaces[0].mode = PortMode::Access { vlan_id: 200 };

    let err = validate_switch_pair_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("undeclared VLAN 200"));
}

#[test]
fn switch_pair_rejects_duplicate_interface_on_same_switch() {
    let mut intent = switch_pair_intent();
    intent.interfaces.push(intent.interfaces[0].clone());

    let err = validate_switch_pair_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("duplicate interface GE1/0/1 on leaf-a"));
}

#[test]
fn domain_rejects_empty_management_endpoint_secret_ref() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.endpoints[0].secret_ref.clear();

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("secret_ref is empty"));
}

#[test]
fn domain_rejects_duplicate_management_endpoint_ids() {
    let mut intent = domain_intent(UnderlayTopology::SmallFabric);
    intent.endpoints.push(intent.endpoints[0].clone());

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("duplicate management endpoint stack-mgmt"));
}

#[test]
fn domain_rejects_mlag_with_one_management_endpoint() {
    let intent = domain_intent(UnderlayTopology::MlagDualManagementIp);

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("mlag topology requires exactly two management endpoints"));
}

#[test]
fn domain_rejects_small_fabric_with_one_management_endpoint() {
    let intent = domain_intent(UnderlayTopology::SmallFabric);

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains(
        "small fabric topology requires at least two management endpoints"
    ));
}

#[test]
fn domain_rejects_trunk_with_duplicate_allowed_vlan() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.vlans.push(VlanIntent {
        vlan_id: 200,
        name: Some("storage".into()),
        description: None,
    });
    intent.interfaces[0].mode = PortMode::Trunk {
        native_vlan: Some(100),
        allowed_vlans: vec![100, 200, 200],
    };

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("duplicate allowed VLAN 200"));
}

#[test]
fn domain_rejects_duplicate_acl_ids() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.acls = vec![acl_intent(3999), acl_intent(3999)];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("duplicate acl_id 3999"));
}

#[test]
fn domain_rejects_acl_port_on_ip_protocol() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    let mut acl = acl_intent(3999);
    acl.rules[0].protocol = AclProtocol::Ip;
    acl.rules[0].destination_port_eq = Some(443);
    intent.acls = vec![acl];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("destination_port_eq but protocol is not tcp/udp"));
}

#[test]
fn domain_rejects_acl_binding_to_undeclared_acl() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.acl_bindings = vec![acl_binding_intent(3999)];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("references undeclared ACL 3999"));
}

#[test]
fn domain_rejects_acl_binding_to_unknown_member() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.acls = vec![acl_intent(3999)];
    intent.acl_bindings = vec![AclBindingIntent {
        device_id: DeviceId("missing-member".into()),
        interface_name: "GE1/0/1".into(),
        direction: AclDirection::Inbound,
        acl_id: 3999,
    }];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("references unknown switch member missing-member"));
}

#[test]
fn domain_rejects_duplicate_acl_binding_direction_on_same_interface() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.acls = vec![acl_intent(3999)];
    intent.acl_bindings = vec![acl_binding_intent(3999), acl_binding_intent(3999)];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("duplicate ACL binding on GE1/0/1"));
}

#[test]
fn domain_accepts_acl_binding_to_declared_acl_and_existing_interface_reference() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.interfaces = vec![];
    intent.acls = vec![acl_intent(3999)];
    intent.acl_bindings = vec![acl_binding_intent(3999)];

    validate_underlay_domain_intent(&intent).expect("declared ACL binding should validate");
}

#[test]
fn domain_rejects_upsert_and_delete_of_same_vlan() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.delete_vlan_ids = vec![100];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("cannot upsert and delete VLAN 100"));
}

#[test]
fn domain_rejects_upsert_and_delete_of_same_acl_binding() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.acls = vec![acl_intent(3999)];
    intent.acl_bindings = vec![acl_binding_intent(3999)];
    intent.delete_acl_bindings = vec![acl_binding_intent(3999)];

    let err = validate_underlay_domain_intent(&intent).unwrap_err();

    assert!(format!("{err}").contains("cannot upsert and delete ACL binding"));
}

#[test]
fn domain_accepts_explicit_delete_intents_for_isolated_targets() {
    let mut intent = domain_intent(UnderlayTopology::StackSingleManagementIp);
    intent.delete_vlan_ids = vec![200];
    intent.delete_acl_ids = vec![3999];
    intent.delete_acl_bindings = vec![acl_binding_intent(3999)];

    validate_underlay_domain_intent(&intent).expect("delete intent should validate");
}

fn switch_pair_intent() -> SwitchPairIntent {
    SwitchPairIntent {
        pair_id: "pair-a".into(),
        switches: vec![
            SwitchIntent {
                device_id: DeviceId("leaf-a".into()),
                role: DeviceRole::LeafA,
            },
            SwitchIntent {
                device_id: DeviceId("leaf-b".into()),
                role: DeviceRole::LeafB,
            },
        ],
        vlans: vec![VlanIntent {
            vlan_id: 100,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![InterfaceIntent {
            device_id: DeviceId("leaf-a".into()),
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: None,
            mode: PortMode::Access { vlan_id: 100 },
        }],
    }
}

fn domain_intent(topology: UnderlayTopology) -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "domain-a".into(),
        topology,
        endpoints: vec![ManagementEndpointIntent {
            endpoint_id: "stack-mgmt".into(),
            host: "127.0.0.1".into(),
            port: 830,
            secret_ref: "local/stack-mgmt".into(),
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
        }],
        members: vec![SwitchMemberIntent {
            member_id: "member-a".into(),
            role: Some(DeviceRole::LeafA),
            management_endpoint_id: "stack-mgmt".into(),
        }],
        vlans: vec![VlanIntent {
            vlan_id: 100,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![InterfaceIntent {
            device_id: DeviceId("member-a".into()),
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: None,
            mode: PortMode::Access { vlan_id: 100 },
        }],
        acls: vec![],
        acl_bindings: vec![],
        delete_vlan_ids: vec![],
        delete_acl_ids: vec![],
        delete_acl_bindings: vec![],
    }
}

fn acl_intent(acl_id: u16) -> AclIntent {
    AclIntent {
        acl_id,
        name: None,
        description: Some("temporary acl".into()),
        rules: vec![AclRule {
            sequence: 10,
            action: AclAction::Permit,
            protocol: AclProtocol::Tcp,
            source: Some(AclEndpoint {
                address: "192.0.2.0".into(),
                wildcard: "0.0.0.255".into(),
            }),
            destination: None,
            source_port_eq: None,
            destination_port_eq: Some(443),
            description: None,
        }],
    }
}

fn acl_binding_intent(acl_id: u16) -> AclBindingIntent {
    AclBindingIntent {
        device_id: DeviceId("member-a".into()),
        interface_name: "GE1/0/1".into(),
        direction: AclDirection::Inbound,
        acl_id,
    }
}
