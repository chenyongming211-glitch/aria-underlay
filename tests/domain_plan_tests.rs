use aria_underlay::intent::interface::{InterfaceDeleteIntent, InterfaceIntent};
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    AclBindingIntent, AclIntent, BgpNeighborDeleteIntent, BgpNeighborIntent,
    BgpProcessDeleteIntent, BgpProcessIntent, ManagementEndpointIntent, SwitchMemberIntent,
    UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{
    AclAction, AclDirection, AclKind, AclProtocol, AclRule, AdminState, DeviceId, DeviceRole,
    PortMode, Vendor,
};
use aria_underlay::planner::domain_plan::plan_underlay_domain;

#[test]
fn stack_single_management_ip_plans_one_endpoint_state() {
    let intent = domain_intent(
        UnderlayTopology::StackSingleManagementIp,
        vec![endpoint("stack-mgmt")],
        vec![
            member("member-a", Some(DeviceRole::LeafA), "stack-mgmt"),
            member("member-b", Some(DeviceRole::LeafB), "stack-mgmt"),
        ],
        vec![
            access_interface("member-a", "GE1/0/1"),
            access_interface("member-b", "GE2/0/1"),
        ],
    );

    let states = plan_underlay_domain(&intent).expect("stack domain should plan");

    assert_eq!(states.len(), 1);
    assert_eq!(states[0].device_id.0, "stack-mgmt");
    assert!(states[0].interfaces.contains_key("GE1/0/1"));
    assert!(states[0].interfaces.contains_key("GE2/0/1"));
}

#[test]
fn mlag_dual_management_ip_plans_two_endpoint_states() {
    let intent = domain_intent(
        UnderlayTopology::MlagDualManagementIp,
        vec![endpoint("leaf-a-mgmt"), endpoint("leaf-b-mgmt")],
        vec![
            member("leaf-a", Some(DeviceRole::LeafA), "leaf-a-mgmt"),
            member("leaf-b", Some(DeviceRole::LeafB), "leaf-b-mgmt"),
        ],
        vec![
            access_interface("leaf-a", "GE1/0/1"),
            access_interface("leaf-b", "GE1/0/1"),
        ],
    );

    let states = plan_underlay_domain(&intent).expect("mlag domain should plan");

    assert_eq!(states.len(), 2);
    assert_eq!(states[0].device_id.0, "leaf-a-mgmt");
    assert_eq!(states[1].device_id.0, "leaf-b-mgmt");
    assert!(states.iter().all(|state| state.vlans.contains_key(&100)));
}

#[test]
fn small_fabric_plans_multiple_endpoint_states() {
    let intent = domain_intent(
        UnderlayTopology::SmallFabric,
        vec![endpoint("sw-1"), endpoint("sw-2"), endpoint("sw-3")],
        vec![
            member("sw-1-member", None, "sw-1"),
            member("sw-2-member", None, "sw-2"),
            member("sw-3-member", None, "sw-3"),
        ],
        vec![
            access_interface("sw-1-member", "GE1/0/1"),
            access_interface("sw-2-member", "GE1/0/1"),
            access_interface("sw-3-member", "GE1/0/1"),
        ],
    );

    let states = plan_underlay_domain(&intent).expect("small fabric should plan");

    assert_eq!(states.len(), 3);
    assert_eq!(states[0].device_id.0, "sw-1");
    assert_eq!(states[1].device_id.0, "sw-2");
    assert_eq!(states[2].device_id.0, "sw-3");
}

#[test]
fn small_fabric_has_no_hardcoded_endpoint_count_limit() {
    let endpoints = (1..=21)
        .map(|index| endpoint(&format!("sw-{index}")))
        .collect::<Vec<_>>();
    let members = (1..=21)
        .map(|index| member(&format!("sw-{index}-member"), None, &format!("sw-{index}")))
        .collect::<Vec<_>>();
    let interfaces = (1..=21)
        .map(|index| access_interface(&format!("sw-{index}-member"), "GE1/0/1"))
        .collect::<Vec<_>>();
    let intent = domain_intent(UnderlayTopology::SmallFabric, endpoints, members, interfaces);

    let states = plan_underlay_domain(&intent).expect("endpoint count should not be hard-limited");

    assert_eq!(states.len(), 21);
}

#[test]
fn acl_intent_is_planned_to_each_management_endpoint() {
    let mut intent = domain_intent(
        UnderlayTopology::MlagDualManagementIp,
        vec![endpoint("leaf-a-mgmt"), endpoint("leaf-b-mgmt")],
        vec![
            member("leaf-a", Some(DeviceRole::LeafA), "leaf-a-mgmt"),
            member("leaf-b", Some(DeviceRole::LeafB), "leaf-b-mgmt"),
        ],
        vec![],
    );
    intent.acls = vec![acl_intent(3999)];

    let states = plan_underlay_domain(&intent).expect("ACL domain should plan");

    assert_eq!(states.len(), 2);
    assert!(states.iter().all(|state| state.acls.contains_key(&3999)));
}

#[test]
fn acl_binding_intent_is_planned_to_owning_management_endpoint() {
    let mut intent = domain_intent(
        UnderlayTopology::MlagDualManagementIp,
        vec![endpoint("leaf-a-mgmt"), endpoint("leaf-b-mgmt")],
        vec![
            member("leaf-a", Some(DeviceRole::LeafA), "leaf-a-mgmt"),
            member("leaf-b", Some(DeviceRole::LeafB), "leaf-b-mgmt"),
        ],
        vec![
            access_interface("leaf-a", "GE1/0/1"),
            access_interface("leaf-b", "GE1/0/1"),
        ],
    );
    intent.acls = vec![acl_intent(3999)];
    intent.acl_bindings = vec![AclBindingIntent {
        device_id: DeviceId("leaf-b".into()),
        interface_name: "GE1/0/1".into(),
        direction: AclDirection::Inbound,
        acl_id: 3999,
    }];

    let states = plan_underlay_domain(&intent).expect("ACL binding domain should plan");

    assert!(states[0].acl_bindings.is_empty());
    assert_eq!(
        states[1].acl_bindings["GE1/0/1|inbound"].acl_id,
        3999
    );
}

#[test]
fn explicit_delete_intents_are_planned_to_target_management_endpoint() {
    let mut intent = domain_intent(
        UnderlayTopology::MlagDualManagementIp,
        vec![endpoint("leaf-a-mgmt"), endpoint("leaf-b-mgmt")],
        vec![
            member("leaf-a", Some(DeviceRole::LeafA), "leaf-a-mgmt"),
            member("leaf-b", Some(DeviceRole::LeafB), "leaf-b-mgmt"),
        ],
        vec![],
    );
    intent.delete_vlan_ids = vec![144];
    intent.delete_interfaces = vec![InterfaceDeleteIntent {
        device_id: DeviceId("leaf-b".into()),
        name: "GE1/0/14".into(),
    }];
    intent.delete_acl_ids = vec![3999];
    intent.delete_acl_bindings = vec![AclBindingIntent {
        device_id: DeviceId("leaf-b".into()),
        interface_name: "GE1/0/13".into(),
        direction: AclDirection::Inbound,
        acl_id: 3999,
    }];

    let states = plan_underlay_domain(&intent).expect("delete domain should plan");

    assert!(states.iter().all(|state| state.delete_vlan_ids.contains(&144)));
    assert!(states.iter().all(|state| state.delete_acl_ids.contains(&3999)));
    assert!(states[0].delete_interface_names.is_empty());
    assert!(states[1].delete_interface_names.contains("GE1/0/14"));
    assert!(states[0].delete_acl_bindings.is_empty());
    assert_eq!(
        states[1].delete_acl_bindings["GE1/0/13|inbound"].acl_id,
        3999
    );
}

#[test]
fn bgp_intents_are_planned_to_owning_management_endpoint() {
    let mut intent = domain_intent(
        UnderlayTopology::MlagDualManagementIp,
        vec![endpoint("leaf-a-mgmt"), endpoint("leaf-b-mgmt")],
        vec![
            member("leaf-a", Some(DeviceRole::LeafA), "leaf-a-mgmt"),
            member("leaf-b", Some(DeviceRole::LeafB), "leaf-b-mgmt"),
        ],
        vec![],
    );
    intent.bgp_processes = vec![BgpProcessIntent {
        device_id: DeviceId("leaf-b".into()),
        vrf: "default".into(),
        local_as: 65_000,
        router_id: Some("192.0.2.1".into()),
    }];
    intent.bgp_neighbors = vec![BgpNeighborIntent {
        device_id: DeviceId("leaf-b".into()),
        vrf: "default".into(),
        address: "203.0.113.10".into(),
        remote_as: 65_001,
        description: Some("tenant-a edge".into()),
        import_policy: Some("RP-IN".into()),
        export_policy: Some("RP-OUT".into()),
    }];
    intent.delete_bgp_processes = vec![BgpProcessDeleteIntent {
        device_id: DeviceId("leaf-a".into()),
        vrf: "blue".into(),
    }];
    intent.delete_bgp_neighbors = vec![BgpNeighborDeleteIntent {
        device_id: DeviceId("leaf-a".into()),
        vrf: "blue".into(),
        address: "198.51.100.20".into(),
    }];

    let states = plan_underlay_domain(&intent).expect("BGP domain should plan");

    assert!(states[0].bgp_processes.is_empty());
    assert!(states[0].delete_bgp_process_vrfs.contains("blue"));
    assert_eq!(
        states[0].delete_bgp_neighbors["blue|198.51.100.20"].address,
        "198.51.100.20"
    );
    assert_eq!(states[1].bgp_processes["default"].local_as, 65_000);
    assert_eq!(
        states[1].bgp_neighbors["default|203.0.113.10"]
            .import_policy
            .as_deref(),
        Some("RP-IN")
    );
}

#[test]
fn unknown_member_reference_fails_validation() {
    let intent = domain_intent(
        UnderlayTopology::SmallFabric,
        vec![endpoint("sw-1"), endpoint("sw-2")],
        vec![
            member("sw-1-member", None, "sw-1"),
            member("sw-2-member", None, "sw-2"),
        ],
        vec![access_interface("missing-member", "GE1/0/1")],
    );

    let err = plan_underlay_domain(&intent).unwrap_err();

    assert!(format!("{err}").contains("unknown switch member missing-member"));
}

fn domain_intent(
    topology: UnderlayTopology,
    endpoints: Vec<ManagementEndpointIntent>,
    members: Vec<SwitchMemberIntent>,
    interfaces: Vec<InterfaceIntent>,
) -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "domain-a".into(),
        topology,
        endpoints,
        members,
        vlans: vec![VlanIntent {
            vlan_id: 100,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces,
        acls: vec![],
        acl_bindings: vec![],
        delete_vlan_ids: vec![],
        delete_interfaces: vec![],
        delete_acl_ids: vec![],
        delete_acl_bindings: vec![],
        bgp_processes: vec![],
        bgp_neighbors: vec![],
        delete_bgp_processes: vec![],
        delete_bgp_neighbors: vec![],
    }
}

fn endpoint(endpoint_id: &str) -> ManagementEndpointIntent {
    ManagementEndpointIntent {
        endpoint_id: endpoint_id.into(),
        host: "127.0.0.1".into(),
        port: 830,
        secret_ref: format!("local/{endpoint_id}"),
        vendor_hint: Some(Vendor::Unknown),
        model_hint: None,
    }
}

fn member(
    member_id: &str,
    role: Option<DeviceRole>,
    management_endpoint_id: &str,
) -> SwitchMemberIntent {
    SwitchMemberIntent {
        member_id: member_id.into(),
        role,
        management_endpoint_id: management_endpoint_id.into(),
    }
}

fn access_interface(member_id: &str, name: &str) -> InterfaceIntent {
    InterfaceIntent {
        device_id: DeviceId(member_id.into()),
        name: name.into(),
        admin_state: AdminState::Up,
        description: None,
        mode: PortMode::Access { vlan_id: 100 },
    }
}

fn acl_intent(acl_id: u16) -> AclIntent {
    AclIntent {
        acl_id,
        kind: AclKind::AdvancedIpv4,
        name: None,
        description: Some("temporary acl".into()),
        rules: vec![AclRule {
            sequence: 10,
            action: AclAction::Permit,
            protocol: AclProtocol::Ip,
            source: None,
            destination: None,
            source_port_eq: None,
            destination_port_eq: None,
            description: None,
        }],
    }
}
