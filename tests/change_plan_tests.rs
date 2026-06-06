use aria_underlay::device::model_profile::{
    DeviceModelProfile, ModelPathSupport, ModelProtocol, WriteReadiness,
};
use aria_underlay::engine::change_plan::{
    build_change_plan, build_change_plan_with_profile, BlastRadius, ChangePlanStageKind,
    DryRunWriteDecision,
};
use aria_underlay::engine::diff::{ChangeOp, ChangeSet};
use aria_underlay::engine::dry_run::{build_dry_run_plan, build_dry_run_plan_with_profiles};
use aria_underlay::model::{
    AclAction, AclBinding, AclConfig, AclDirection, AclKind, AclProtocol, AclRule, BgpNeighbor,
    BgpProcess, DeviceId, Vendor, VlanConfig,
};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::state::DeviceShadowState;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn change_plan_orders_acl_before_acl_binding_on_create() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![
            ChangeOp::CreateAclBinding(acl_binding()),
            ChangeOp::CreateAcl(acl_config()),
        ],
    };

    let plan = build_change_plan(&change_set);

    assert_eq!(plan.stages[0].kind, ChangePlanStageKind::CreateBaseObjects);
    assert_eq!(plan.stages[1].kind, ChangePlanStageKind::BindReferences);
    assert_eq!(
        plan.dependency_edges[0].from,
        "acl-binding GigabitEthernet1/0/1 inbound acl 3001"
    );
    assert_eq!(plan.dependency_edges[0].to, "acl 3001");
    assert_eq!(
        plan.rollback_order,
        vec![
            "remove acl binding 3001 on GigabitEthernet1/0/1 inbound",
            "delete acl 3001",
        ]
    );
    assert_eq!(plan.blast_radius, BlastRadius::PolicyReference);
}

#[test]
fn change_plan_orders_unbind_before_acl_delete() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![
            ChangeOp::DeleteAcl { acl_id: 3001 },
            ChangeOp::DeleteAclBinding {
                interface_name: "GigabitEthernet1/0/1".to_string(),
                direction: AclDirection::Inbound,
                acl_id: 3001,
            },
        ],
    };

    let plan = build_change_plan(&change_set);

    assert_eq!(plan.stages[0].kind, ChangePlanStageKind::UnbindReferences);
    assert_eq!(plan.stages[1].kind, ChangePlanStageKind::DeleteBaseObjects);
    assert_eq!(plan.dependency_edges[0].from, "acl 3001 delete");
    assert_eq!(
        plan.dependency_edges[0].to,
        "acl-binding GigabitEthernet1/0/1 inbound acl 3001 unbind"
    );
    assert_eq!(
        plan.rollback_order,
        vec![
            "restore acl 3001",
            "restore acl binding 3001 on GigabitEthernet1/0/1 inbound",
        ]
    );
    assert_eq!(plan.blast_radius, BlastRadius::PolicyReference);
}

#[test]
fn change_plan_treats_interface_config_delete_as_local_delete() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![ChangeOp::DeleteInterfaceConfig {
            interface_name: "GE1/0/13".to_string(),
        }],
    };

    let plan = build_change_plan(&change_set);

    assert_eq!(plan.stages[0].kind, ChangePlanStageKind::DeleteBaseObjects);
    assert_eq!(plan.rollback_order, vec!["restore interface GE1/0/13"]);
    assert_eq!(plan.blast_radius, BlastRadius::LocalInterfaceOrVlan);
}

#[test]
fn dry_run_builds_change_plan_alongside_change_set() {
    let desired = vec![DeviceDesiredState {
        device_id: DeviceId("leaf-1".to_string()),
        vlans: [(100, vlan(100))].into_iter().collect(),
        interfaces: Default::default(),
        acls: Default::default(),
        acl_bindings: Default::default(),
        delete_vlan_ids: Default::default(),
        delete_interface_names: Default::default(),
        delete_acl_ids: Default::default(),
        delete_acl_bindings: Default::default(),
    }];
    let current = vec![DeviceShadowState {
        device_id: DeviceId("leaf-1".to_string()),
        revision: 1,
        vlans: Default::default(),
        interfaces: Default::default(),
        acls: Default::default(),
        acl_bindings: Default::default(),
        warnings: vec![],
    }];

    let plan = build_dry_run_plan(&desired, &current).expect("dry-run should build");

    assert_eq!(plan.change_sets.len(), 1);
    assert_eq!(plan.change_plans.len(), 1);
    assert_eq!(plan.change_plans[0].device_id, "leaf-1");
    assert_eq!(plan.change_plans[0].blast_radius, BlastRadius::LocalInterfaceOrVlan);
    assert_eq!(
        plan.change_plans[0].stages[0].kind,
        ChangePlanStageKind::CreateBaseObjects
    );
}

#[test]
fn dry_run_rejects_bgp_neighbor_without_path_level_profile_evidence() {
    let mut desired_state = empty_desired_state("leaf-1");
    desired_state.bgp_processes.insert(
        "default".to_string(),
        BgpProcess {
            vrf: "default".to_string(),
            local_as: 65_000,
            router_id: Some("192.0.2.1".to_string()),
        },
    );
    desired_state.bgp_neighbors.insert(
        "default|203.0.113.10".to_string(),
        BgpNeighbor {
            vrf: "default".to_string(),
            address: "203.0.113.10".to_string(),
            remote_as: 65_001,
            description: Some("tenant-a edge".to_string()),
            import_policy: Some("RP-IN".to_string()),
            export_policy: Some("RP-OUT".to_string()),
        },
    );
    let current_state = empty_shadow_state("leaf-1");
    let profiles = BTreeMap::from([(
        DeviceId("leaf-1".to_string()),
        DeviceModelProfile {
            profile_id: "h3c:S5560:Comware7".to_string(),
            vendor: Vendor::H3c,
            model: "S5560".to_string(),
            os_version: "Comware7".to_string(),
            paths: vec![],
            pbr_write_readiness: WriteReadiness::WriteRejected,
            bgp_write_readiness: WriteReadiness::WriteRejected,
            rejection_reasons: vec!["bgp: no path-level write evidence".to_string()],
            yang_module_count: 0,
        },
    )]);

    let plan = build_dry_run_plan_with_profiles(&[desired_state], &[current_state], &profiles)
        .expect("dry-run should build");

    assert_eq!(
        plan.change_sets[0].ops,
        vec![
            ChangeOp::CreateBgpProcess(BgpProcess {
                vrf: "default".to_string(),
                local_as: 65_000,
                router_id: Some("192.0.2.1".to_string()),
            }),
            ChangeOp::CreateBgpNeighbor(BgpNeighbor {
                vrf: "default".to_string(),
                address: "203.0.113.10".to_string(),
                remote_as: 65_001,
                description: Some("tenant-a edge".to_string()),
                import_policy: Some("RP-IN".to_string()),
                export_policy: Some("RP-OUT".to_string()),
            }),
        ]
    );
    assert_eq!(plan.change_plans[0].blast_radius, BlastRadius::RoutingControlPlane);
    assert_eq!(plan.change_plans[0].write_decision, DryRunWriteDecision::Rejected);
    assert_eq!(
        plan.change_plans[0].unsupported_paths,
        vec!["bgp: no path-level write evidence".to_string()]
    );
    assert_eq!(plan.change_plans[0].dependency_edges[0].from, "bgp-neighbor default 203.0.113.10");
    assert_eq!(plan.change_plans[0].dependency_edges[0].to, "bgp-process default");
    assert_eq!(
        plan.change_plans[0].rollback_order,
        vec![
            "delete bgp neighbor default 203.0.113.10",
            "delete bgp process default",
        ]
    );
}

fn acl_binding() -> AclBinding {
    AclBinding {
        interface_name: "GigabitEthernet1/0/1".to_string(),
        direction: AclDirection::Inbound,
        acl_id: 3001,
    }
}

fn acl_config() -> AclConfig {
    AclConfig {
        acl_id: 3001,
        kind: AclKind::AdvancedIpv4,
        name: None,
        description: Some("tenant guard".to_string()),
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

fn vlan(vlan_id: u16) -> VlanConfig {
    VlanConfig {
        vlan_id,
        name: Some("tenant".to_string()),
        description: None,
    }
}

fn empty_desired_state(device_id: &str) -> DeviceDesiredState {
    DeviceDesiredState {
        device_id: DeviceId(device_id.to_string()),
        vlans: BTreeMap::new(),
        interfaces: BTreeMap::new(),
        acls: BTreeMap::new(),
        acl_bindings: BTreeMap::new(),
        bgp_processes: BTreeMap::new(),
        bgp_neighbors: BTreeMap::new(),
        delete_vlan_ids: BTreeSet::new(),
        delete_interface_names: BTreeSet::new(),
        delete_acl_ids: BTreeSet::new(),
        delete_acl_bindings: BTreeMap::new(),
        delete_bgp_process_vrfs: BTreeSet::new(),
        delete_bgp_neighbors: BTreeMap::new(),
    }
}

fn empty_shadow_state(device_id: &str) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId(device_id.to_string()),
        revision: 1,
        vlans: BTreeMap::new(),
        interfaces: BTreeMap::new(),
        acls: BTreeMap::new(),
        acl_bindings: BTreeMap::new(),
        bgp_processes: BTreeMap::new(),
        bgp_neighbors: BTreeMap::new(),
        warnings: vec![],
    }
}

#[test]
fn change_plan_without_profile_defaults_to_vendor_private_and_empty_unsupported() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![ChangeOp::CreateVlan(vlan(100))],
    };

    let plan = build_change_plan(&change_set);

    assert_eq!(plan.write_decision, DryRunWriteDecision::AllowedVendorPrivate);
    assert!(plan.unsupported_paths.is_empty());
}

#[test]
fn change_plan_with_write_rejected_profile_reports_unsupported_paths() {
    let profile = DeviceModelProfile {
        profile_id: "h3c:S5560:Comware7".to_string(),
        vendor: Vendor::H3c,
        model: "S5560".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![],
        pbr_write_readiness: WriteReadiness::WriteRejected,
        bgp_write_readiness: WriteReadiness::WriteRejected,
        rejection_reasons: vec![
            "pbr: no path-level write evidence".to_string(),
            "bgp: no path-level writing evidence".to_string(),
        ],
        yang_module_count: 0,
    };
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![ChangeOp::CreateAcl(acl_config())],
    };

    let plan = build_change_plan_with_profile(&change_set, Some(&profile));

    assert_eq!(plan.unsupported_paths.len(), 2);
    assert!(plan.unsupported_paths[0].starts_with("pbr:"));
    assert!(plan.unsupported_paths[1].starts_with("bgp:"));
    assert_eq!(plan.write_decision, DryRunWriteDecision::Rejected);
}

#[test]
fn change_plan_with_write_safe_profile_reports_standard_model_decision() {
    let profile = DeviceModelProfile {
        profile_id: "h3c:S6800:Comware7".to_string(),
        vendor: Vendor::H3c,
        model: "S6800".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![ModelPathSupport {
            protocol: ModelProtocol::OpenConfigNetconf,
            model: "openconfig-vlan".to_string(),
            revision: Some("2024-01-15".to_string()),
            path: "/vlans".to_string(),
            readable: true,
            writable: true,
            verified_on_device: true,
            deviations: vec![],
            notes: vec![],
        }],
        pbr_write_readiness: WriteReadiness::WriteSafe,
        bgp_write_readiness: WriteReadiness::WriteSafe,
        rejection_reasons: vec![],
        yang_module_count: 0,
    };
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![ChangeOp::CreateVlan(vlan(100))],
    };

    let plan = build_change_plan_with_profile(&change_set, Some(&profile));

    assert!(plan.unsupported_paths.is_empty());
    assert_eq!(plan.write_decision, DryRunWriteDecision::AllowedStandardModel);
}

#[test]
fn change_plan_with_read_only_profile_reports_read_only_for_policy_changes() {
    let profile = DeviceModelProfile {
        profile_id: "h3c:S5560:Comware7".to_string(),
        vendor: Vendor::H3c,
        model: "S5560".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![],
        pbr_write_readiness: WriteReadiness::ReadOnly,
        bgp_write_readiness: WriteReadiness::ReadOnly,
        rejection_reasons: vec![],
        yang_module_count: 0,
    };
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![ChangeOp::CreateAcl(acl_config())],
    };

    let plan = build_change_plan_with_profile(&change_set, Some(&profile));

    assert_eq!(plan.write_decision, DryRunWriteDecision::ReadOnly);
    assert!(plan.unsupported_paths.is_empty());
}
