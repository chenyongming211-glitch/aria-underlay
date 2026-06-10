use serde::{Deserialize, Serialize};

use crate::device::model_profile::{DeviceModelProfile, WriteReadiness};
use crate::engine::diff::{ChangeOp, ChangeSet};
use crate::model::AclDirection;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlan {
    pub device_id: String,
    #[serde(default)]
    pub high_risk_features: Vec<HighRiskFeature>,
    pub stages: Vec<ChangePlanStage>,
    pub dependency_edges: Vec<ChangeDependencyEdge>,
    pub rollback_order: Vec<String>,
    pub blast_radius: BlastRadius,
    #[serde(default)]
    pub unsupported_paths: Vec<String>,
    pub write_decision: DryRunWriteDecision,
}

/// Per-device dry-run write decision.
///
/// This is the final write decision for the current change set, derived from the
/// device's model profile. Current production features (VLAN, interface, ACL) use
/// handwritten vendor renderers and are allowed through the low-risk path.
/// High-risk features (PBR, BGP, QoS, NQA) must be declared explicitly and
/// require path-level evidence in the model profile before writes are permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DryRunWriteDecision {
    /// Write allowed via standard model (OpenConfig/gNMI or OpenConfig-over-NETCONF).
    AllowedStandardModel,
    /// Write allowed via vendor native YANG with path-level evidence.
    AllowedVendorNative,
    /// Write allowed via vendor private renderer (current production surface).
    /// Used when no model profile is available or when the change set only touches
    /// features covered by handwritten renderers.
    AllowedVendorPrivate,
    /// Only read-only or audit operations are permitted for high-risk features.
    ReadOnly,
    /// Write rejected: device lacks required transaction support or path evidence.
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighRiskFeature {
    Pbr,
    Bgp,
    Qos,
    Nqa,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlanStage {
    pub kind: ChangePlanStageKind,
    pub ops: Vec<ChangeOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeDependencyEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangePlanStageKind {
    UnbindReferences,
    DeleteBaseObjects,
    CreateBaseObjects,
    UpdateBaseObjects,
    BindReferences,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlastRadius {
    NoChange,
    LocalInterfaceOrVlan,
    PolicyReference,
    RoutingControlPlane,
}

pub fn build_change_plan(change_set: &ChangeSet) -> ChangePlan {
    build_change_plan_with_profile(change_set, None)
}

pub fn build_change_plan_with_profile(
    change_set: &ChangeSet,
    profile: Option<&DeviceModelProfile>,
) -> ChangePlan {
    build_change_plan_with_profile_and_high_risk_features(change_set, profile, &[])
}

pub fn build_change_plan_with_profile_and_high_risk_features(
    change_set: &ChangeSet,
    profile: Option<&DeviceModelProfile>,
    high_risk_features: &[HighRiskFeature],
) -> ChangePlan {
    let mut unbind = Vec::new();
    let mut delete_base = Vec::new();
    let mut create_base = Vec::new();
    let mut update_base = Vec::new();
    let mut bind = Vec::new();
    for op in &change_set.ops {
        match op {
            ChangeOp::DeleteAclBinding { .. } => unbind.push(op.clone()),
            ChangeOp::DeleteAcl { .. }
            | ChangeOp::DeleteVlan { .. }
            | ChangeOp::DeleteInterfaceConfig { .. } => delete_base.push(op.clone()),
            ChangeOp::CreateAcl(_) | ChangeOp::CreateVlan(_) => create_base.push(op.clone()),
            ChangeOp::UpdateAcl { .. }
            | ChangeOp::UpdateVlan { .. }
            | ChangeOp::UpdateInterface { .. } => update_base.push(op.clone()),
            ChangeOp::CreateAclBinding(_) | ChangeOp::UpdateAclBinding { .. } => {
                bind.push(op.clone())
            }
        }
    }

    let mut stages = Vec::new();
    push_stage(&mut stages, ChangePlanStageKind::UnbindReferences, unbind);
    push_stage(&mut stages, ChangePlanStageKind::DeleteBaseObjects, delete_base);
    push_stage(&mut stages, ChangePlanStageKind::CreateBaseObjects, create_base);
    push_stage(&mut stages, ChangePlanStageKind::UpdateBaseObjects, update_base);
    push_stage(&mut stages, ChangePlanStageKind::BindReferences, bind);

    let dependency_edges = dependency_edges_for_change_set(change_set);
    let rollback_order = rollback_order_for_stages(&stages);
    let blast_radius = classify_blast_radius(change_set, high_risk_features);
    let unsupported_paths = collect_unsupported_paths(profile, high_risk_features);
    let write_decision = classify_write_decision(profile, high_risk_features);
    ChangePlan {
        device_id: change_set.device_id.0.clone(),
        high_risk_features: high_risk_features.to_vec(),
        stages,
        dependency_edges,
        rollback_order,
        blast_radius,
        unsupported_paths,
        write_decision,
    }
}

/// Collect paths that the desired change set touches but the device profile does
/// not support for writing. Current production features (VLAN, interface, ACL)
/// are served by handwritten vendor renderers and never appear here. Only
/// high-risk features that require model profile evidence are checked.
fn collect_unsupported_paths(
    profile: Option<&DeviceModelProfile>,
    high_risk_features: &[HighRiskFeature],
) -> Vec<String> {
    if high_risk_features.is_empty() {
        return Vec::new();
    }
    let Some(profile) = profile else {
        return high_risk_features
            .iter()
            .map(|feature| format!("{}: missing device model profile", feature.name()))
            .collect();
    };
    let mut unsupported = Vec::new();
    for feature in high_risk_features {
        match feature_readiness(profile, *feature) {
            Some(WriteReadiness::WriteSafe) => {
                if !has_verified_writable_path_for_feature(profile, *feature) {
                    unsupported.push(format!(
                        "{}: missing verified writable path-level evidence",
                        feature.name()
                    ));
                }
            }
            Some(WriteReadiness::ReadOnly) => unsupported.push(format!(
                "{}: write readiness is read-only",
                feature.name()
            )),
            Some(WriteReadiness::WriteRejected) => {
                unsupported.push(format_rejection_reason(profile, *feature))
            }
            None => unsupported.push(format!(
                "{}: DeviceModelProfile write readiness is not implemented",
                feature.name()
            )),
        }
    }
    unsupported
}

fn first_rejection_reason(profile: &DeviceModelProfile, feature: &str) -> String {
    profile
        .rejection_reasons
        .iter()
        .find(|reason| reason.to_lowercase().contains(feature))
        .cloned()
        .unwrap_or_else(|| format!("write rejected for {feature}"))
}

fn format_rejection_reason(profile: &DeviceModelProfile, feature: HighRiskFeature) -> String {
    let reason = first_rejection_reason(profile, feature.name());
    let feature_prefix = format!("{}:", feature.name());
    if reason.to_lowercase().starts_with(&feature_prefix) {
        reason
    } else {
        format!("{}: {reason}", feature.name())
    }
}

/// Derive the final write decision for this dry-run from the device model profile
/// and the explicitly declared high-risk features.
///
/// - No profile: vendor private renderer is assumed for the current production
///   surface (VLAN, interface, ACL).
/// - Declared high-risk feature without profile evidence: rejected.
/// - Profile with `WriteRejected` readiness for a declared high-risk feature:
///   rejected.
/// - Profile with `ReadOnly` readiness: read-only.
/// - Profile with `WriteSafe` readiness: standard model or vendor native depending
///   on which paths have evidence.
fn classify_write_decision(
    profile: Option<&DeviceModelProfile>,
    high_risk_features: &[HighRiskFeature],
) -> DryRunWriteDecision {
    if high_risk_features.is_empty() {
        return classify_low_risk_write_decision(profile);
    }

    let Some(profile) = profile else {
        return DryRunWriteDecision::Rejected;
    };

    if high_risk_features
        .iter()
        .any(|feature| high_risk_feature_is_rejected(profile, *feature))
    {
        return DryRunWriteDecision::Rejected;
    }

    if high_risk_features
        .iter()
        .any(|feature| feature_readiness(profile, *feature) == Some(WriteReadiness::ReadOnly))
    {
        return DryRunWriteDecision::ReadOnly;
    }

    classify_writable_path_decision(profile)
}

fn classify_low_risk_write_decision(profile: Option<&DeviceModelProfile>) -> DryRunWriteDecision {
    let Some(profile) = profile else {
        return DryRunWriteDecision::AllowedVendorPrivate;
    };
    classify_writable_path_decision(profile)
}

fn classify_writable_path_decision(profile: &DeviceModelProfile) -> DryRunWriteDecision {
    if profile
        .paths
        .iter()
        .any(|path| path.verified_on_device && path.writable && path.readable)
    {
        let has_openconfig = profile.paths.iter().any(|path| {
            path.verified_on_device
                && path.writable
                && matches!(
                    path.protocol,
                    crate::device::model_profile::ModelProtocol::OpenConfigGnmi
                        | crate::device::model_profile::ModelProtocol::OpenConfigNetconf
                )
        });
        if has_openconfig {
            DryRunWriteDecision::AllowedStandardModel
        } else {
            DryRunWriteDecision::AllowedVendorNative
        }
    } else {
        DryRunWriteDecision::AllowedVendorPrivate
    }
}

fn high_risk_feature_is_rejected(profile: &DeviceModelProfile, feature: HighRiskFeature) -> bool {
    match feature_readiness(profile, feature) {
        Some(WriteReadiness::WriteSafe) => !has_verified_writable_path_for_feature(profile, feature),
        Some(WriteReadiness::ReadOnly) => false,
        Some(WriteReadiness::WriteRejected) | None => true,
    }
}

fn feature_readiness(
    profile: &DeviceModelProfile,
    feature: HighRiskFeature,
) -> Option<WriteReadiness> {
    match feature {
        HighRiskFeature::Pbr => Some(profile.pbr_write_readiness),
        HighRiskFeature::Bgp => Some(profile.bgp_write_readiness),
        HighRiskFeature::Qos | HighRiskFeature::Nqa => None,
    }
}

fn has_verified_writable_path_for_feature(
    profile: &DeviceModelProfile,
    feature: HighRiskFeature,
) -> bool {
    profile.paths.iter().any(|path| {
        path.readable
            && path.writable
            && path.verified_on_device
            && feature.matches_path(&path.path)
    })
}

impl HighRiskFeature {
    fn name(self) -> &'static str {
        match self {
            Self::Pbr => "pbr",
            Self::Bgp => "bgp",
            Self::Qos => "qos",
            Self::Nqa => "nqa",
        }
    }

    fn matches_path(self, path: &str) -> bool {
        let path = path.to_lowercase();
        match self {
            Self::Pbr => path.contains("policy-forwarding") || path.contains("pbr"),
            Self::Bgp => path.contains("bgp"),
            Self::Qos => {
                path.contains("qos")
                    || path.contains("quality-of-service")
                    || path.contains("traffic")
            }
            Self::Nqa => path.contains("nqa"),
        }
    }
}

fn push_stage(stages: &mut Vec<ChangePlanStage>, kind: ChangePlanStageKind, ops: Vec<ChangeOp>) {
    if !ops.is_empty() {
        stages.push(ChangePlanStage { kind, ops });
    }
}

fn dependency_edges_for_change_set(change_set: &ChangeSet) -> Vec<ChangeDependencyEdge> {
    let mut edges = Vec::new();
    for op in &change_set.ops {
        match op {
            ChangeOp::CreateAclBinding(binding)
            | ChangeOp::UpdateAclBinding { after: binding, .. } => edges.push(
                ChangeDependencyEdge {
                    from: acl_binding_node(
                        &binding.interface_name,
                        &binding.direction,
                        binding.acl_id,
                        "",
                    ),
                    to: acl_node(binding.acl_id, ""),
                },
            ),
            ChangeOp::DeleteAcl { acl_id } => {
                let binding_edges = delete_acl_binding_edges(change_set, *acl_id);
                if binding_edges.is_empty() {
                    edges.push(ChangeDependencyEdge {
                        from: acl_node(*acl_id, "delete"),
                        to: format!("all acl {acl_id} bindings unbound"),
                    });
                } else {
                    edges.extend(binding_edges);
                }
            }
            _ => {}
        }
    }
    edges
}

fn delete_acl_binding_edges(change_set: &ChangeSet, acl_id: u16) -> Vec<ChangeDependencyEdge> {
    change_set
        .ops
        .iter()
        .filter_map(|op| match op {
            ChangeOp::DeleteAclBinding {
                interface_name,
                direction,
                acl_id: binding_acl_id,
            } if *binding_acl_id == acl_id => Some(ChangeDependencyEdge {
                from: acl_node(acl_id, "delete"),
                to: acl_binding_node(
                    interface_name,
                    direction,
                    acl_id,
                    "unbind",
                ),
            }),
            _ => None,
        })
        .collect()
}

fn rollback_order_for_stages(stages: &[ChangePlanStage]) -> Vec<String> {
    let mut rollback_order = Vec::new();
    for stage in stages.iter().rev() {
        for op in stage.ops.iter().rev() {
            rollback_order.push(rollback_action_for_op(op));
        }
    }
    rollback_order
}

fn rollback_action_for_op(op: &ChangeOp) -> String {
    match op {
        ChangeOp::CreateVlan(vlan) => format!("delete vlan {}", vlan.vlan_id),
        ChangeOp::UpdateVlan { before, .. } => format!("restore vlan {}", before.vlan_id),
        ChangeOp::DeleteVlan { vlan_id } => format!("restore vlan {vlan_id}"),
        ChangeOp::UpdateInterface { after, .. } => format!("restore interface {}", after.name),
        ChangeOp::DeleteInterfaceConfig { interface_name } => {
            format!("restore interface {interface_name}")
        }
        ChangeOp::CreateAcl(acl) => format!("delete acl {}", acl.acl_id),
        ChangeOp::UpdateAcl { before, .. } => format!("restore acl {}", before.acl_id),
        ChangeOp::DeleteAcl { acl_id } => format!("restore acl {acl_id}"),
        ChangeOp::CreateAclBinding(binding) | ChangeOp::UpdateAclBinding { after: binding, .. } => {
            format!(
                "remove acl binding {} on {} {}",
                binding.acl_id,
                binding.interface_name,
                acl_direction_text(&binding.direction)
            )
        }
        ChangeOp::DeleteAclBinding {
            interface_name,
            direction,
            acl_id,
        } => format!(
            "restore acl binding {acl_id} on {interface_name} {}",
            acl_direction_text(direction)
        ),
    }
}

fn classify_blast_radius(
    change_set: &ChangeSet,
    high_risk_features: &[HighRiskFeature],
) -> BlastRadius {
    if high_risk_features
        .iter()
        .any(|feature| *feature == HighRiskFeature::Bgp)
    {
        return BlastRadius::RoutingControlPlane;
    }
    if !high_risk_features.is_empty() {
        return BlastRadius::PolicyReference;
    }
    if change_set.ops.is_empty() {
        return BlastRadius::NoChange;
    }
    if change_set.ops.iter().any(|op| {
        matches!(
            op,
            ChangeOp::CreateAcl(_)
                | ChangeOp::UpdateAcl { .. }
                | ChangeOp::DeleteAcl { .. }
                | ChangeOp::CreateAclBinding(_)
                | ChangeOp::UpdateAclBinding { .. }
                | ChangeOp::DeleteAclBinding { .. }
        )
    }) {
        return BlastRadius::PolicyReference;
    }
    BlastRadius::LocalInterfaceOrVlan
}

fn acl_node(acl_id: u16, suffix: &str) -> String {
    match suffix {
        "" => format!("acl {acl_id}"),
        _ => format!("acl {acl_id} {suffix}"),
    }
}

fn acl_binding_node(
    interface_name: &str,
    direction: &AclDirection,
    acl_id: u16,
    suffix: &str,
) -> String {
    let node = format!(
        "acl-binding {interface_name} {} acl {acl_id}",
        acl_direction_text(direction)
    );
    match suffix {
        "" => node,
        _ => format!("{node} {suffix}"),
    }
}

fn acl_direction_text(direction: &AclDirection) -> &'static str {
    match direction {
        AclDirection::Inbound => "inbound",
        AclDirection::Outbound => "outbound",
    }
}
