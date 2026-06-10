use aria_underlay::device::model_profile::{
    DeviceModelProfile, ModelPathSupport, ModelProtocol, WriteReadiness,
};
use aria_underlay::engine::change_plan::{
    build_change_plan, build_change_plan_with_profile,
    build_change_plan_with_profile_and_high_risk_features, BlastRadius, ChangePlanStageKind,
    DryRunWriteDecision, HighRiskFeature,
};
use aria_underlay::engine::diff::{ChangeOp, ChangeSet};
use aria_underlay::engine::dry_run::build_dry_run_plan;
use aria_underlay::model::{
    AclAction, AclBinding, AclConfig, AclDirection, AclKind, AclProtocol, AclRule, DeviceId, Vendor,
    VlanConfig,
};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::state::DeviceShadowState;

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
fn low_risk_acl_change_is_not_blocked_by_unrelated_high_risk_profile_rejections() {
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

    assert!(plan.high_risk_features.is_empty());
    assert!(plan.unsupported_paths.is_empty());
    assert_eq!(plan.write_decision, DryRunWriteDecision::AllowedVendorPrivate);
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
fn low_risk_acl_change_remains_vendor_private_with_read_only_high_risk_profile() {
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

    assert!(plan.high_risk_features.is_empty());
    assert_eq!(plan.write_decision, DryRunWriteDecision::AllowedVendorPrivate);
    assert!(plan.unsupported_paths.is_empty());
}

#[test]
fn high_risk_bgp_write_without_profile_is_rejected_with_routing_blast_radius() {
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![],
    };

    let plan = build_change_plan_with_profile_and_high_risk_features(
        &change_set,
        None,
        &[HighRiskFeature::Bgp],
    );

    assert_eq!(plan.high_risk_features, vec![HighRiskFeature::Bgp]);
    assert_eq!(plan.blast_radius, BlastRadius::RoutingControlPlane);
    assert_eq!(plan.write_decision, DryRunWriteDecision::Rejected);
    assert_eq!(
        plan.unsupported_paths,
        vec!["bgp: missing device model profile"]
    );
}

#[test]
fn high_risk_pbr_write_rejected_profile_reports_pbr_reason_only() {
    let profile = DeviceModelProfile {
        profile_id: "h3c:S5560:Comware7".to_string(),
        vendor: Vendor::H3c,
        model: "S5560".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![],
        pbr_write_readiness: WriteReadiness::WriteRejected,
        bgp_write_readiness: WriteReadiness::WriteSafe,
        rejection_reasons: vec![
            "pbr: no path-level write evidence".to_string(),
            "bgp: verified on OpenConfig".to_string(),
        ],
        yang_module_count: 0,
    };
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![],
    };

    let plan = build_change_plan_with_profile_and_high_risk_features(
        &change_set,
        Some(&profile),
        &[HighRiskFeature::Pbr],
    );

    assert_eq!(plan.high_risk_features, vec![HighRiskFeature::Pbr]);
    assert_eq!(plan.blast_radius, BlastRadius::PolicyReference);
    assert_eq!(plan.write_decision, DryRunWriteDecision::Rejected);
    assert_eq!(
        plan.unsupported_paths,
        vec!["pbr: no path-level write evidence"]
    );
}

#[test]
fn high_risk_bgp_read_only_profile_reports_read_only_decision() {
    let profile = DeviceModelProfile {
        profile_id: "h3c:S5560:Comware7".to_string(),
        vendor: Vendor::H3c,
        model: "S5560".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![ModelPathSupport {
            protocol: ModelProtocol::OpenConfigNetconf,
            model: "openconfig-bgp".to_string(),
            revision: Some("2024-10-30".to_string()),
            path: "/network-instances/network-instance/protocols/protocol/bgp".to_string(),
            readable: true,
            writable: false,
            verified_on_device: true,
            deviations: vec![],
            notes: vec!["readback only".to_string()],
        }],
        pbr_write_readiness: WriteReadiness::WriteSafe,
        bgp_write_readiness: WriteReadiness::ReadOnly,
        rejection_reasons: vec![],
        yang_module_count: 1,
    };
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![],
    };

    let plan = build_change_plan_with_profile_and_high_risk_features(
        &change_set,
        Some(&profile),
        &[HighRiskFeature::Bgp],
    );

    assert_eq!(plan.write_decision, DryRunWriteDecision::ReadOnly);
    assert_eq!(
        plan.unsupported_paths,
        vec!["bgp: write readiness is read-only"]
    );
}

#[test]
fn high_risk_qos_and_nqa_default_to_rejected_until_profile_contract_exists() {
    let profile = DeviceModelProfile {
        profile_id: "h3c:S5560:Comware7".to_string(),
        vendor: Vendor::H3c,
        model: "S5560".to_string(),
        os_version: "Comware7".to_string(),
        paths: vec![],
        pbr_write_readiness: WriteReadiness::WriteSafe,
        bgp_write_readiness: WriteReadiness::WriteSafe,
        rejection_reasons: vec![],
        yang_module_count: 0,
    };
    let change_set = ChangeSet {
        device_id: DeviceId("leaf-1".to_string()),
        ops: vec![],
    };

    let plan = build_change_plan_with_profile_and_high_risk_features(
        &change_set,
        Some(&profile),
        &[HighRiskFeature::Qos, HighRiskFeature::Nqa],
    );

    assert_eq!(plan.blast_radius, BlastRadius::PolicyReference);
    assert_eq!(plan.write_decision, DryRunWriteDecision::Rejected);
    assert_eq!(
        plan.unsupported_paths,
        vec![
            "qos: DeviceModelProfile write readiness is not implemented",
            "nqa: DeviceModelProfile write readiness is not implemented",
        ]
    );
}
