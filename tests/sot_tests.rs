use aria_underlay::sot::snapshot::{
    SotAcl, SotBgpNeighbor, SotDevice, SotInterface, SotPolicyIntent, SotSnapshot, SotSource,
    SotVlan,
};

#[test]
fn sot_snapshot_rejects_duplicate_device_ids() {
    let mut snapshot = base_snapshot();
    snapshot.devices.push(snapshot.devices[0].clone());

    let err = snapshot.validate().unwrap_err();

    assert_eq!(err, "duplicate SoT device_id leaf-1");
}

#[test]
fn sot_snapshot_rejects_inventory_for_unknown_device() {
    let mut snapshot = base_snapshot();
    snapshot.interfaces.push(SotInterface {
        device_id: "leaf-missing".to_string(),
        name: "GigabitEthernet1/0/2".to_string(),
        description: None,
        source: source(),
    });

    let err = snapshot.validate().unwrap_err();

    assert_eq!(
        err,
        "SoT interface GigabitEthernet1/0/2 references unknown device_id leaf-missing"
    );
}

#[test]
fn sot_snapshot_rejects_duplicate_device_interface_names() {
    let mut snapshot = base_snapshot();
    snapshot.interfaces.push(SotInterface {
        device_id: "leaf-1".to_string(),
        name: "GigabitEthernet1/0/1".to_string(),
        description: Some("duplicate".to_string()),
        source: source(),
    });

    let err = snapshot.validate().unwrap_err();

    assert_eq!(
        err,
        "duplicate SoT interface leaf-1/GigabitEthernet1/0/1"
    );
}

#[test]
fn sot_snapshot_rejects_empty_ownership_metadata() {
    let mut snapshot = base_snapshot();
    snapshot.vlans[0].owner = " ".to_string();

    let err = snapshot.validate().unwrap_err();

    assert_eq!(err, "SoT vlan 100 owner must not be empty");
}

#[test]
fn sot_snapshot_rejects_empty_source_metadata() {
    let mut snapshot = base_snapshot();
    snapshot.devices[0].source.system = " ".to_string();

    let err = snapshot.validate().unwrap_err();

    assert_eq!(err, "SoT device leaf-1 source system must not be empty");
}

#[test]
fn sot_snapshot_expresses_policy_and_bgp_inputs_without_external_connector_types() {
    let snapshot = base_snapshot();

    snapshot.validate().unwrap();
    assert_eq!(
        snapshot.devices[0].model_profile_ref.as_deref(),
        Some("h3c:s5560:v7")
    );
    assert_eq!(snapshot.policy_intents[0].owner, "tenant-a");
    assert_eq!(snapshot.bgp_neighbors[0].remote_as, 65_001);
    assert_eq!(snapshot.bgp_neighbors[0].source.system, "file");
}

fn base_snapshot() -> SotSnapshot {
    SotSnapshot {
        devices: vec![SotDevice {
            device_id: "leaf-1".to_string(),
            vendor: "h3c".to_string(),
            model: "S5560".to_string(),
            os_version: "Comware7".to_string(),
            model_profile_ref: Some("h3c:s5560:v7".to_string()),
            source: source(),
        }],
        interfaces: vec![SotInterface {
            device_id: "leaf-1".to_string(),
            name: "GigabitEthernet1/0/1".to_string(),
            description: Some("tenant uplink".to_string()),
            source: source(),
        }],
        vlans: vec![SotVlan {
            device_id: "leaf-1".to_string(),
            vlan_id: 100,
            name: Some("tenant-a".to_string()),
            owner: "tenant-a".to_string(),
            source: source(),
        }],
        acls: vec![SotAcl {
            device_id: "leaf-1".to_string(),
            acl_id: 3001,
            owner: "tenant-a".to_string(),
            source: source(),
        }],
        policy_intents: vec![SotPolicyIntent {
            device_id: "leaf-1".to_string(),
            policy_id: "pbr-tenant-a".to_string(),
            owner: "tenant-a".to_string(),
            source: source(),
        }],
        bgp_neighbors: vec![SotBgpNeighbor {
            device_id: "leaf-1".to_string(),
            vrf: "default".to_string(),
            neighbor_address: "192.0.2.2".to_string(),
            remote_as: 65_001,
            owner: "tenant-a".to_string(),
            source: source(),
        }],
    }
}

fn source() -> SotSource {
    SotSource {
        system: "file".to_string(),
        reference: "fixtures/sot/tenant-a.yaml".to_string(),
    }
}
