use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::device::model_profile::{DeviceModelProfile, ModelPathSupport, WriteReadiness};
use crate::engine::diff::{ChangeOp, ChangeSet};
use crate::model::AclDirection;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlan {
    pub device_id: String,
    pub stages: Vec<ChangePlanStage>,
    pub dependency_edges: Vec<ChangeDependencyEdge>,
    #[serde(default)]
    pub route_policy_dependencies: Vec<RoutePolicyDependency>,
    #[serde(default)]
    pub missing_route_policy_refs: Vec<String>,
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
/// handwritten vendor renderers and are always allowed. High-risk features (PBR,
/// BGP, QoS) require path-level evidence in the model profile before writes are
/// permitted.
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
pub struct RoutePolicyDependency {
    pub neighbor: String,
    pub policy: String,
    pub direction: RoutePolicyDirection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutePolicyDirection {
    Import,
    Export,
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
    build_change_plan_with_profile_and_route_policy_evidence(
        change_set,
        profile,
        &BTreeSet::new(),
    )
}

pub fn build_change_plan_with_profile_and_route_policy_evidence(
    change_set: &ChangeSet,
    profile: Option<&DeviceModelProfile>,
    route_policy_evidence: &BTreeSet<String>,
) -> ChangePlan {
    let mut unbind = Vec::new();
    let mut delete_base = Vec::new();
    let mut create_base = Vec::new();
    let mut update_base = Vec::new();
    let mut bind = Vec::new();
    for op in &change_set.ops {
        match op {
            ChangeOp::DeleteAclBinding { .. } => unbind.push(op.clone()),
            ChangeOp::DeleteBgpNeighbor { .. } => unbind.push(op.clone()),
            ChangeOp::DeleteAcl { .. }
            | ChangeOp::DeleteVlan { .. }
            | ChangeOp::DeleteInterfaceConfig { .. }
            | ChangeOp::DeleteBgpProcess { .. } => delete_base.push(op.clone()),
            ChangeOp::CreateAcl(_) | ChangeOp::CreateVlan(_) | ChangeOp::CreateBgpProcess(_) => {
                create_base.push(op.clone())
            }
            ChangeOp::UpdateAcl { .. }
            | ChangeOp::UpdateVlan { .. }
            | ChangeOp::UpdateInterface { .. }
            | ChangeOp::UpdateBgpProcess { .. } => update_base.push(op.clone()),
            ChangeOp::CreateAclBinding(_)
            | ChangeOp::UpdateAclBinding { .. }
            | ChangeOp::CreateBgpNeighbor(_)
            | ChangeOp::UpdateBgpNeighbor { .. } => bind.push(op.clone()),
        }
    }

    let mut stages = Vec::new();
    push_stage(&mut stages, ChangePlanStageKind::UnbindReferences, unbind);
    push_stage(&mut stages, ChangePlanStageKind::DeleteBaseObjects, delete_base);
    push_stage(&mut stages, ChangePlanStageKind::CreateBaseObjects, create_base);
    push_stage(&mut stages, ChangePlanStageKind::UpdateBaseObjects, update_base);
    push_stage(&mut stages, ChangePlanStageKind::BindReferences, bind);

    let route_policy_dependencies = route_policy_dependencies_for_change_set(change_set);
    let missing_route_policy_refs = missing_route_policy_refs(
        &route_policy_dependencies,
        route_policy_evidence,
    );
    let dependency_edges = dependency_edges_for_change_set(change_set);
    let rollback_order = rollback_order_for_stages(&stages);
    let blast_radius = classify_blast_radius(change_set);
    let unsupported_paths = collect_unsupported_paths(change_set, profile, &missing_route_policy_refs);
    let write_decision =
        classify_write_decision(profile, &blast_radius, &unsupported_paths, change_set);
    ChangePlan {
        device_id: change_set.device_id.0.clone(),
        stages,
        dependency_edges,
        route_policy_dependencies,
        missing_route_policy_refs,
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
    change_set: &ChangeSet,
    profile: Option<&DeviceModelProfile>,
    missing_route_policy_refs: &[String],
) -> Vec<String> {
    let mut unsupported = missing_route_policy_refs
        .iter()
        .map(|policy| format!("bgp: missing route-policy evidence {policy}"))
        .collect::<Vec<_>>();

    let Some(profile) = profile else {
        if touches_bgp(change_set) {
            unsupported.push("bgp: missing device model profile".to_string());
            return unsupported;
        }
        return unsupported;
    };
    if touches_pbr(change_set) && profile.pbr_write_readiness == WriteReadiness::WriteRejected {
        unsupported.push(feature_rejection_reason(profile, "pbr"));
    }
    if touches_bgp(change_set) {
        if profile.bgp_write_readiness == WriteReadiness::WriteRejected {
            unsupported.push(feature_rejection_reason(profile, "bgp"));
        } else if profile.bgp_write_readiness == WriteReadiness::WriteSafe
            && !has_writable_bgp_path(profile)
        {
            unsupported.push("bgp: missing writable BGP path evidence".to_string());
        }
    }
    unsupported
}

fn feature_rejection_reason(profile: &DeviceModelProfile, feature: &str) -> String {
    let reason = first_rejection_reason(profile, feature);
    let prefix = format!("{feature}:");
    if reason.trim_start().to_ascii_lowercase().starts_with(&prefix) {
        reason
    } else {
        format!("{feature}: {reason}")
    }
}

fn first_rejection_reason(profile: &DeviceModelProfile, feature: &str) -> String {
    profile
        .rejection_reasons
        .iter()
        .find(|reason| reason.to_lowercase().contains(feature))
        .cloned()
        .unwrap_or_else(|| format!("write rejected for {feature}"))
}

/// Derive the final write decision for this dry-run from the device model profile,
/// the blast radius of the change set, and any unsupported paths.
///
/// - No profile: vendor private renderer is assumed for the current production
///   surface (VLAN, interface, ACL).
/// - Profile with `WriteRejected` readiness on a high-risk blast radius: rejected.
/// - Profile with `ReadOnly` readiness: read-only.
/// - Profile with `WriteSafe` readiness: standard model or vendor native depending
///   on which paths have evidence.
fn classify_write_decision(
    profile: Option<&DeviceModelProfile>,
    blast_radius: &BlastRadius,
    unsupported_paths: &[String],
    change_set: &ChangeSet,
) -> DryRunWriteDecision {
    let Some(profile) = profile else {
        if matches!(blast_radius, BlastRadius::RoutingControlPlane) {
            return DryRunWriteDecision::Rejected;
        }
        return DryRunWriteDecision::AllowedVendorPrivate;
    };

    if matches!(
        blast_radius,
        BlastRadius::RoutingControlPlane | BlastRadius::PolicyReference
    ) && !unsupported_paths.is_empty()
    {
        return DryRunWriteDecision::Rejected;
    }

    if profile.pbr_write_readiness == WriteReadiness::WriteRejected
        && profile.bgp_write_readiness == WriteReadiness::WriteRejected
    {
        if matches!(blast_radius, BlastRadius::RoutingControlPlane) {
            return DryRunWriteDecision::Rejected;
        }
    }

    if (touches_pbr(change_set) && profile.pbr_write_readiness == WriteReadiness::ReadOnly)
        || (touches_bgp(change_set) && profile.bgp_write_readiness == WriteReadiness::ReadOnly)
    {
        if matches!(
            blast_radius,
            BlastRadius::RoutingControlPlane | BlastRadius::PolicyReference
        ) {
            return DryRunWriteDecision::ReadOnly;
        }
    }

    let writable_paths = profile
        .paths
        .iter()
        .filter(|path| !touches_bgp(change_set) || is_bgp_path(path))
        .filter(|path| path.verified_on_device && path.writable && path.readable)
        .collect::<Vec<_>>();

    if !writable_paths.is_empty() {
        let has_openconfig = writable_paths.iter().any(|path| {
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

fn has_writable_bgp_path(profile: &DeviceModelProfile) -> bool {
    profile
        .paths
        .iter()
        .any(|path| is_bgp_path(path) && path.verified_on_device && path.writable && path.readable)
}

fn is_bgp_path(path: &ModelPathSupport) -> bool {
    let model = path.model.to_ascii_lowercase();
    let path_text = path.path.to_ascii_lowercase();
    model.contains("bgp") || path_text.contains("bgp")
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
            ChangeOp::CreateBgpNeighbor(neighbor)
            | ChangeOp::UpdateBgpNeighbor {
                after: neighbor, ..
            } => {
                let neighbor_node = bgp_neighbor_node(&neighbor.vrf, &neighbor.address);
                edges.push(ChangeDependencyEdge {
                    from: neighbor_node.clone(),
                    to: bgp_process_node(&neighbor.vrf),
                });
                if let Some(policy) = normalized_policy_ref(&neighbor.import_policy) {
                    edges.push(ChangeDependencyEdge {
                        from: neighbor_node.clone(),
                        to: route_policy_node(&policy),
                    });
                }
                if let Some(policy) = normalized_policy_ref(&neighbor.export_policy) {
                    edges.push(ChangeDependencyEdge {
                        from: neighbor_node,
                        to: route_policy_node(&policy),
                    });
                }
            }
            ChangeOp::DeleteBgpProcess { vrf } => {
                let neighbor_edges = delete_bgp_neighbor_edges(change_set, vrf);
                if neighbor_edges.is_empty() {
                    edges.push(ChangeDependencyEdge {
                        from: format!("{} delete", bgp_process_node(vrf)),
                        to: format!("all bgp process {vrf} neighbors removed"),
                    });
                } else {
                    edges.extend(neighbor_edges);
                }
            }
            _ => {}
        }
    }
    edges
}

fn route_policy_dependencies_for_change_set(change_set: &ChangeSet) -> Vec<RoutePolicyDependency> {
    let mut dependencies = Vec::new();
    for op in &change_set.ops {
        match op {
            ChangeOp::CreateBgpNeighbor(neighbor)
            | ChangeOp::UpdateBgpNeighbor {
                after: neighbor, ..
            } => {
                let neighbor_node = bgp_neighbor_node(&neighbor.vrf, &neighbor.address);
                if let Some(policy) = normalized_policy_ref(&neighbor.import_policy) {
                    dependencies.push(RoutePolicyDependency {
                        neighbor: neighbor_node.clone(),
                        policy,
                        direction: RoutePolicyDirection::Import,
                    });
                }
                if let Some(policy) = normalized_policy_ref(&neighbor.export_policy) {
                    dependencies.push(RoutePolicyDependency {
                        neighbor: neighbor_node,
                        policy,
                        direction: RoutePolicyDirection::Export,
                    });
                }
            }
            _ => {}
        }
    }
    dependencies
}

fn missing_route_policy_refs(
    dependencies: &[RoutePolicyDependency],
    route_policy_evidence: &BTreeSet<String>,
) -> Vec<String> {
    dependencies
        .iter()
        .map(|dependency| dependency.policy.trim().to_string())
        .filter(|policy| !policy.is_empty())
        .filter(|policy| !route_policy_evidence.contains(policy))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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

fn delete_bgp_neighbor_edges(change_set: &ChangeSet, vrf: &str) -> Vec<ChangeDependencyEdge> {
    change_set
        .ops
        .iter()
        .filter_map(|op| match op {
            ChangeOp::DeleteBgpNeighbor {
                vrf: neighbor_vrf,
                address,
            } if neighbor_vrf == vrf => Some(ChangeDependencyEdge {
                from: format!("{} delete", bgp_process_node(vrf)),
                to: format!("{} delete", bgp_neighbor_node(vrf, address)),
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
        ChangeOp::CreateBgpProcess(process) => format!("delete bgp process {}", process.vrf),
        ChangeOp::UpdateBgpProcess { before, .. } => {
            format!("restore bgp process {}", before.vrf)
        }
        ChangeOp::DeleteBgpProcess { vrf } => format!("restore bgp process {vrf}"),
        ChangeOp::CreateBgpNeighbor(neighbor) => {
            format!("delete bgp neighbor {} {}", neighbor.vrf, neighbor.address)
        }
        ChangeOp::UpdateBgpNeighbor { before, .. } => {
            format!("restore bgp neighbor {} {}", before.vrf, before.address)
        }
        ChangeOp::DeleteBgpNeighbor { vrf, address } => {
            format!("restore bgp neighbor {vrf} {address}")
        }
    }
}

fn classify_blast_radius(change_set: &ChangeSet) -> BlastRadius {
    if change_set.ops.is_empty() {
        return BlastRadius::NoChange;
    }
    if touches_bgp(change_set) {
        return BlastRadius::RoutingControlPlane;
    }
    if touches_acl(change_set) {
        return BlastRadius::PolicyReference;
    }
    BlastRadius::LocalInterfaceOrVlan
}

fn touches_acl(change_set: &ChangeSet) -> bool {
    change_set.ops.iter().any(|op| {
        matches!(
            op,
            ChangeOp::CreateAcl(_)
                | ChangeOp::UpdateAcl { .. }
                | ChangeOp::DeleteAcl { .. }
                | ChangeOp::CreateAclBinding(_)
                | ChangeOp::UpdateAclBinding { .. }
                | ChangeOp::DeleteAclBinding { .. }
        )
    })
}

fn touches_pbr(_change_set: &ChangeSet) -> bool {
    false
}

fn touches_bgp(change_set: &ChangeSet) -> bool {
    change_set.ops.iter().any(|op| {
        matches!(
            op,
            ChangeOp::CreateBgpProcess(_)
                | ChangeOp::UpdateBgpProcess { .. }
                | ChangeOp::DeleteBgpProcess { .. }
                | ChangeOp::CreateBgpNeighbor(_)
                | ChangeOp::UpdateBgpNeighbor { .. }
                | ChangeOp::DeleteBgpNeighbor { .. }
        )
    })
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

fn bgp_process_node(vrf: &str) -> String {
    format!("bgp-process {vrf}")
}

fn bgp_neighbor_node(vrf: &str, address: &str) -> String {
    format!("bgp-neighbor {vrf} {address}")
}

fn route_policy_node(policy: &str) -> String {
    format!("route-policy {policy}")
}

fn normalized_policy_ref(policy: &Option<String>) -> Option<String> {
    let policy = policy.as_deref()?.trim();
    if policy.is_empty() {
        None
    } else {
        Some(policy.to_string())
    }
}
