use aria_underlay::adapter_client::mapper::{
    adapter_result_to_outcome, capability_from_proto, desired_state_to_proto, extract_adapter_errors,
    device_ref_from_info, recovery_action_to_proto, shadow_state_from_proto,
    state_scope_from_change_set, state_scope_from_desired, AdapterOperationStatus,
};
use aria_underlay::device::{DeviceInfo, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::engine::diff::{ChangeOp, ChangeSet};
use aria_underlay::model::{
    AclAction, AclBinding, AclConfig, AclDirection, AclEndpoint, AclKind, AclProtocol, AclRule,
    AdminState, DeviceId, DeviceRole, InterfaceConfig, PortMode, Vendor, VlanConfig,
};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::proto::adapter;
use aria_underlay::tx::RecoveryAction;
use aria_underlay::UnderlayError;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn maps_host_key_policy_to_device_ref() {
    let known_hosts = DeviceInfo {
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        id: DeviceId("leaf-a".into()),
        management_ip: "192.0.2.10".into(),
        management_port: 830,
        vendor_hint: Some(Vendor::Huawei),
        model_hint: None,
        role: DeviceRole::LeafA,
        secret_ref: "local/leaf-a".into(),
        host_key_policy: HostKeyPolicy::KnownHostsFile {
            path: "/etc/aria/known_hosts".into(),
        },
        adapter_endpoint: "http://127.0.0.1:50051".into(),
        lifecycle_state: DeviceLifecycleState::Ready,
    };

    let known_hosts_ref = device_ref_from_info(&known_hosts);

    assert_eq!(
        known_hosts_ref.host_key_policy,
        adapter::HostKeyPolicy::KnownHostsFile as i32
    );
    assert_eq!(known_hosts_ref.known_hosts_path, "/etc/aria/known_hosts");
    assert_eq!(known_hosts_ref.pinned_host_key_fingerprint, "");

    let pinned = DeviceInfo {
        host_key_policy: HostKeyPolicy::PinnedKey {
            fingerprint: "SHA256:abc123".into(),
        },
        ..known_hosts
    };

    let pinned_ref = device_ref_from_info(&pinned);

    assert_eq!(
        pinned_ref.host_key_policy,
        adapter::HostKeyPolicy::PinnedKey as i32
    );
    assert_eq!(pinned_ref.known_hosts_path, "");
    assert_eq!(pinned_ref.pinned_host_key_fingerprint, "SHA256:abc123");
}

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
            model_profile: None,
            yang_modules: vec![],
        },
        vec!["capability warning".into()],
    );

    assert_eq!(capability.warnings, vec!["capability warning"]);
}

#[test]
fn maps_capability_model_profile() {
    let capability = capability_from_proto(
        adapter::DeviceCapability {
            vendor: adapter::Vendor::H3c as i32,
            model: "S5560".into(),
            os_version: "Comware7".into(),
            raw_capabilities: vec![],
            supports_netconf: true,
            supports_candidate: true,
            supports_validate: true,
            supports_confirmed_commit: false,
            supports_persist_id: false,
            supports_rollback_on_error: false,
            supports_writable_running: false,
            supported_backends: vec![adapter::BackendKind::Netconf as i32],
            model_profile: Some(adapter::DeviceModelProfile {
                profile_id: "h3c:S5560:Comware7".into(),
                vendor: adapter::Vendor::H3c as i32,
                model: "S5560".into(),
                os_version: "Comware7".into(),
                paths: vec![],
                pbr_write_readiness: adapter::WriteReadiness::WriteRejected as i32,
                bgp_write_readiness: adapter::WriteReadiness::ReadOnly as i32,
                rejection_reasons: vec!["bgp path is read-only".into()],
                yang_module_count: 0,
            }),
            yang_modules: vec![],
        },
        vec![],
    );

    let model_profile = capability
        .model_profile
        .expect("capability model profile should map");
    assert_eq!(model_profile.profile_id, "h3c:S5560:Comware7");
    assert_eq!(
        model_profile.bgp_write_readiness,
        aria_underlay::device::model_profile::WriteReadiness::ReadOnly
    );
    assert_eq!(
        model_profile.rejection_reasons,
        vec!["bgp path is read-only".to_string()]
    );
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
            acls: vec![adapter::AclConfig {
                acl_id: 3999,
                kind: adapter::AclKind::AdvancedIpv4 as i32,
                name: None,
                description: Some("temporary acl".into()),
                rules: vec![adapter::AclRule {
                    sequence: 10,
                    action: adapter::AclAction::Permit as i32,
                    protocol: adapter::AclProtocol::Ip as i32,
                    source: Some(adapter::AclEndpoint {
                        address: "192.0.2.1".into(),
                        wildcard: "0.0.0.0".into(),
                    }),
                    destination: None,
                    source_port_eq: None,
                    destination_port_eq: None,
                    description: None,
                }],
            }],
            acl_bindings: vec![adapter::AclBinding {
                interface_name: "GE1/0/1".into(),
                direction: adapter::AclDirection::Inbound as i32,
                acl_id: 3999,
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
    assert_eq!(shadow.acls[&3999].description.as_deref(), Some("temporary acl"));
    assert_eq!(shadow.acls[&3999].kind, AclKind::AdvancedIpv4);
    assert_eq!(shadow.acls[&3999].rules[0].action, AclAction::Permit);
    assert_eq!(
        shadow.acl_bindings["GE1/0/1|inbound"],
        AclBinding {
            interface_name: "GE1/0/1".into(),
            direction: AclDirection::Inbound,
            acl_id: 3999,
        }
    );
}

#[test]
fn rejects_observed_acl_kind_outside_its_numeric_range() {
    let error = shadow_state_from_proto(
        adapter::ObservedDeviceState {
            device_id: "leaf-a".into(),
            vlans: vec![],
            interfaces: vec![],
            acls: vec![adapter::AclConfig {
                acl_id: 2001,
                kind: adapter::AclKind::AdvancedIpv4 as i32,
                name: None,
                description: None,
                rules: vec![],
            }],
            acl_bindings: vec![],
        },
        vec![],
    )
    .expect_err("advanced ACL kind must not be accepted in the basic ACL range");

    assert!(format!("{error}").contains("INVALID_ACL_KIND"));
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
        acls: BTreeMap::from([(3999, acl(3999))]),
        acl_bindings: BTreeMap::from([(
            "GE1/0/1|inbound".into(),
            acl_binding("GE1/0/1", AclDirection::Inbound, 3999),
        )]),
        delete_vlan_ids: BTreeSet::from([300]),
        delete_interface_names: BTreeSet::from(["GE1/0/3".into()]),
        delete_acl_ids: BTreeSet::from([3998]),
        delete_acl_bindings: BTreeMap::from([(
            "GE1/0/2|outbound".into(),
            acl_binding("GE1/0/2", AclDirection::Outbound, 3998),
        )]),
    };

    let proto = desired_state_to_proto(&desired);

    assert_eq!(proto.device_id, "leaf-a");
    assert_eq!(proto.vlans[0].vlan_id, 100);
    assert_eq!(proto.interfaces[0].mode.as_ref().unwrap().allowed_vlans, vec![100, 200]);
    assert_eq!(proto.acls[0].acl_id, 3999);
    assert_eq!(proto.acls[0].kind, adapter::AclKind::AdvancedIpv4 as i32);
    assert_eq!(proto.acls[0].rules[0].protocol, adapter::AclProtocol::Tcp as i32);
    assert_eq!(proto.acls[0].rules[0].destination_port_eq, Some(443));
    assert_eq!(proto.acl_bindings[0].interface_name, "GE1/0/1");
    assert_eq!(
        proto.acl_bindings[0].direction,
        adapter::AclDirection::Inbound as i32
    );
    assert_eq!(proto.acl_bindings[0].acl_id, 3999);
    assert_eq!(proto.delete_vlan_ids, vec![300]);
    assert_eq!(proto.delete_interface_names, vec!["GE1/0/3"]);
    assert_eq!(proto.delete_acl_ids, vec![3998]);
    assert_eq!(proto.delete_acl_bindings[0].interface_name, "GE1/0/2");
    assert_eq!(proto.delete_acl_bindings[0].acl_id, 3998);
}

#[test]
fn maps_recovery_actions_to_proto() {
    assert_eq!(
        recovery_action_to_proto(RecoveryAction::DiscardPreparedChanges),
        adapter::RecoveryAction::DiscardPreparedChanges
    );
    assert_eq!(
        recovery_action_to_proto(RecoveryAction::AdapterRecover),
        adapter::RecoveryAction::AdapterRecover
    );
    assert_eq!(
        recovery_action_to_proto(RecoveryAction::ManualIntervention),
        adapter::RecoveryAction::Unspecified
    );
}

#[test]
fn derives_state_scope_from_desired_state() {
    let mut desired = desired_state();
    desired.delete_vlan_ids.insert(200);
    desired.delete_interface_names.insert("GE1/0/3".into());
    desired.delete_acl_ids.insert(3998);
    let delete_binding = acl_binding("GE1/0/2", AclDirection::Outbound, 3998);
    desired
        .delete_acl_bindings
        .insert(delete_binding.key(), delete_binding);

    let scope = state_scope_from_desired(&desired);

    assert!(!scope.full);
    assert_eq!(scope.vlan_ids, vec![100, 200]);
    assert_eq!(scope.interface_names, vec!["GE1/0/1", "GE1/0/2", "GE1/0/3"]);
    assert_eq!(scope.acl_ids, vec![3998, 3999]);
}

#[test]
fn derives_state_scope_from_change_set_including_deletes() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-a".into()),
        ops: vec![
            ChangeOp::UpdateVlan {
                before: VlanConfig {
                    vlan_id: 100,
                    name: Some("old".into()),
                    description: None,
                },
                after: VlanConfig {
                    vlan_id: 100,
                    name: Some("new".into()),
                    description: None,
                },
            },
            ChangeOp::DeleteVlan { vlan_id: 200 },
            ChangeOp::UpdateInterface {
                before: Some(InterfaceConfig {
                    name: "GE1/0/1".into(),
                    admin_state: AdminState::Up,
                    description: None,
                    mode: PortMode::Access { vlan_id: 100 },
                }),
                after: InterfaceConfig {
                    name: "GE1/0/1".into(),
                    admin_state: AdminState::Up,
                    description: Some("server".into()),
                    mode: PortMode::Access { vlan_id: 100 },
                },
            },
            ChangeOp::DeleteInterfaceConfig {
                interface_name: "GE1/0/2".into(),
            },
            ChangeOp::CreateAcl(acl(3999)),
            ChangeOp::DeleteAcl { acl_id: 3998 },
            ChangeOp::CreateAclBinding(acl_binding(
                "GE1/0/3",
                AclDirection::Inbound,
                3999,
            )),
            ChangeOp::DeleteAclBinding {
                interface_name: "GE1/0/4".into(),
                direction: AclDirection::Outbound,
                acl_id: 3998,
            },
        ],
    };

    let scope = state_scope_from_change_set(&change_set);

    assert!(!scope.full);
    assert_eq!(scope.vlan_ids, vec![100, 200]);
    assert_eq!(
        scope.interface_names,
        vec!["GE1/0/1", "GE1/0/2", "GE1/0/3", "GE1/0/4"]
    );
    assert_eq!(scope.acl_ids, vec![3998, 3999]);
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
        prepared_candidate_checksum: "sha256:test".into(),
    })
    .expect("result should map");

    assert_eq!(outcome.status, AdapterOperationStatus::Prepared);
    assert!(outcome.changed);
    assert_eq!(outcome.warnings, vec!["degraded"]);
    assert_eq!(
        outcome.prepared_candidate_checksum.as_deref(),
        Some("sha256:test")
    );
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
        prepared_candidate_checksum: String::new(),
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
        acls: BTreeMap::from([(3999, acl(3999))]),
        acl_bindings: BTreeMap::from([(
            "GE1/0/1|inbound".into(),
            acl_binding("GE1/0/1", AclDirection::Inbound, 3999),
        )]),
        delete_vlan_ids: Default::default(),
        delete_interface_names: Default::default(),
        delete_acl_ids: Default::default(),
        delete_acl_bindings: Default::default(),
    }
}

fn acl(acl_id: u16) -> AclConfig {
    AclConfig {
        acl_id,
        kind: AclKind::AdvancedIpv4,
        name: None,
        description: Some("temporary acl".into()),
        rules: vec![AclRule {
            sequence: 10,
            action: AclAction::Deny,
            protocol: AclProtocol::Tcp,
            source: Some(AclEndpoint {
                address: "192.0.2.0".into(),
                wildcard: "0.0.0.255".into(),
            }),
            destination: Some(AclEndpoint {
                address: "198.51.100.10".into(),
                wildcard: "0.0.0.0".into(),
            }),
            source_port_eq: None,
            destination_port_eq: Some(443),
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
