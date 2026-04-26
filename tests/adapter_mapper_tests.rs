use aria_underlay::adapter_client::mapper::{
    adapter_result_to_outcome, capability_from_proto, desired_state_to_proto, extract_adapter_errors,
    shadow_state_from_proto, state_scope_from_desired, AdapterOperationStatus,
};
use aria_underlay::model::{AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::proto::adapter;
use aria_underlay::UnderlayError;
use std::collections::BTreeMap;

#[test]
fn maps_capability_warnings() {
    let capability = capability_from_proto(
        adapter::DeviceCapability {
            vendor: adapter::Vendor::Unknown as i32,
            model: "fake".into(),
            os_version: "1.0".into(),
            raw_capabilities: vec![],
            supports_netconf: true,
            supports_candidate: true,
            supports_validate: true,
            supports_confirmed_commit: true,
            supports_persist_id: true,
            supports_rollback_on_error: false,
            supports_writable_running: false,
            supported_backends: vec![adapter::BackendKind::Netconf as i32],
        },
        vec!["capability warning".into()],
    );

    assert_eq!(capability.warnings, vec!["capability warning"]);
}

#[test]
fn extracts_all_adapter_errors() {
    let error = extract_adapter_errors(vec![
        adapter::AdapterError {
            code: "FIRST".into(),
            message: "first error".into(),
            normalized_error: String::new(),
            raw_error_summary: String::new(),
            retryable: true,
        },
        adapter::AdapterError {
            code: "SECOND".into(),
            message: "second error".into(),
            normalized_error: String::new(),
            raw_error_summary: String::new(),
            retryable: false,
        },
    ])
    .expect("adapter errors should map");

    match error {
        UnderlayError::AdapterOperation {
            code,
            message,
            retryable,
            errors,
        } => {
            assert_eq!(code, "FIRST");
            assert_eq!(message, "first error");
            assert!(retryable);
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].code, "SECOND");
            assert_eq!(errors[0].message, "second error");
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn maps_observed_state_to_shadow_state() {
    let shadow = shadow_state_from_proto(
        adapter::ObservedDeviceState {
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
        },
        vec!["test warning".into()],
    )
    .expect("observed state should map");

    assert_eq!(shadow.device_id.0, "leaf-a");
    assert_eq!(shadow.vlans[&100].name.as_deref(), Some("prod"));
    assert_eq!(shadow.warnings, vec!["test warning"]);
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
fn derives_state_scope_from_desired_state() {
    let desired = desired_state();

    let scope = state_scope_from_desired(&desired);

    assert!(!scope.full);
    assert_eq!(scope.vlan_ids, vec![100]);
    assert_eq!(scope.interface_names, vec!["GE1/0/1"]);
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

fn desired_state() -> DeviceDesiredState {
    DeviceDesiredState {
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
                mode: PortMode::Access { vlan_id: 100 },
            },
        )]),
    }
}
