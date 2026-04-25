use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{AdminState, DeviceId, DeviceRole, PortMode, Vendor};
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
fn unknown_member_reference_fails_validation() {
    let intent = domain_intent(
        UnderlayTopology::SmallFabric,
        vec![endpoint("sw-1")],
        vec![member("sw-1-member", None, "sw-1")],
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
