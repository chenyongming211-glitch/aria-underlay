use aria_underlay::adapter_client::mapper::{
    adapter_result_to_outcome, desired_state_to_proto, shadow_state_from_proto,
    AdapterOperationStatus,
};
use aria_underlay::model::{AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::proto::adapter;
use std::collections::BTreeMap;

#[test]
fn maps_observed_state_to_shadow_state() {
    let shadow = shadow_state_from_proto(adapter::ObservedDeviceState {
        device_id: "leaf-a".into(),
        vlans: vec![adapter::VlanConfig {
            vlan_id: 100,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![adapter::InterfaceConfig {
            name: "GE1/0/1".into(),
            admin_state: adapter::AdminState::Up as i32,
            description: Some("server uplink".into()),
            mode: Some(adapter::PortMode {
                kind: adapter::PortModeKind::Access as i32,
                access_vlan: Some(100),
                native_vlan: None,
                allowed_vlans: vec![],
            }),
        }],
    })
    .expect("observed state should map");

    assert_eq!(shadow.device_id.0, "leaf-a");
    assert_eq!(shadow.vlans[&100].name.as_deref(), Some("prod"));
    assert!(matches!(
        shadow.interfaces["GE1/0/1"].mode,
        PortMode::Access { vlan_id: 100 }
    ));
}

#[test]
fn maps_desired_state_to_proto() {
    let desired = DeviceDesiredState {
        device_id: DeviceId("leaf-a".into()),
        vlans: BTreeMap::from([(
            100,
            VlanConfig {
                vlan_id: 100,
                name: Some("prod".into()),
                description: None,
            },
        )]),
        interfaces: BTreeMap::from([(
            "GE1/0/1".into(),
            InterfaceConfig {
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Trunk {
                    native_vlan: Some(100),
                    allowed_vlans: vec![100, 200],
                },
            },
        )]),
    };

    let proto = desired_state_to_proto(&desired);

    assert_eq!(proto.device_id, "leaf-a");
    assert_eq!(proto.vlans[0].vlan_id, 100);
    assert_eq!(proto.interfaces[0].mode.as_ref().unwrap().allowed_vlans, vec![100, 200]);
}

#[test]
fn maps_adapter_result_success() {
    let outcome = adapter_result_to_outcome(adapter::AdapterResult {
        status: adapter::AdapterOperationStatus::Prepared as i32,
        changed: true,
        warnings: vec!["degraded".into()],
        errors: vec![],
        rollback_artifact: None,
        normalized_state: None,
    })
    .expect("result should map");

    assert_eq!(outcome.status, AdapterOperationStatus::Prepared);
    assert!(outcome.changed);
    assert_eq!(outcome.warnings, vec!["degraded"]);
}

#[test]
fn maps_adapter_result_error() {
    let error = adapter_result_to_outcome(adapter::AdapterResult {
        status: adapter::AdapterOperationStatus::Failed as i32,
        changed: false,
        warnings: vec![],
        errors: vec![adapter::AdapterError {
            code: "LOCK_FAILED".into(),
            message: "lock failed".into(),
            normalized_error: "candidate lock failed".into(),
            raw_error_summary: "mock".into(),
            retryable: true,
        }],
        rollback_artifact: None,
        normalized_state: None,
    })
    .expect_err("adapter error should map to UnderlayError");

    assert!(format!("{error}").contains("LOCK_FAILED"));
}

