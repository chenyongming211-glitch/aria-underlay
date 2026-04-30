use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::validation::{
    validate_switch_pair_intent, validate_underlay_domain_intent,
};
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    ManagementEndpointIntent, SwitchIntent, SwitchMemberIntent, SwitchPairIntent,
    UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{AdminState, DeviceId, DeviceRole, PortMode, Vendor};

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
    }
}
