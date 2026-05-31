use serde::{Deserialize, Serialize};

use crate::engine::diff::{ChangeOp, ChangeSet};
use crate::model::AclDirection;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePlan {
    pub device_id: String,
    pub stages: Vec<ChangePlanStage>,
    pub dependency_edges: Vec<ChangeDependencyEdge>,
    pub rollback_order: Vec<String>,
    pub blast_radius: BlastRadius,
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
    ChangePlan {
        device_id: change_set.device_id.0.clone(),
        stages,
        dependency_edges,
        rollback_order,
        blast_radius: classify_blast_radius(change_set),
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
        ChangeOp::DeleteInterfaceConfig { name } => format!("restore interface {name}"),
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

fn classify_blast_radius(change_set: &ChangeSet) -> BlastRadius {
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
