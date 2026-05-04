use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RbacRole {
    Viewer,
    Operator,
    BreakGlassOperator,
    Admin,
    Auditor,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AdminAction {
    ListOperationSummaries,
    ListAlerts,
    ListInDoubtTransactions,
    AcknowledgeAlert,
    ResolveAlert,
    SuppressAlert,
    ExpireAlert,
    ForceResolveTransaction,
    ForceUnlockSession,
    ChangeRetentionPolicy,
    ChangeDaemonSchedule,
    GetProductStatusBundle,
    GetWorkerReloadStatus,
    ExportAuditHistory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub request_id: String,
    pub trace_id: String,
    pub operator_id: String,
    pub action: AdminAction,
}

impl AuthorizationRequest {
    pub fn new(
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
        operator_id: impl Into<String>,
        action: AdminAction,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            operator_id: operator_id.into(),
            action,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationDecision {
    pub operator_id: String,
    pub role: RbacRole,
    pub action: AdminAction,
}

pub trait AuthorizationPolicy: std::fmt::Debug + Send + Sync {
    fn authorize(&self, request: &AuthorizationRequest) -> UnderlayResult<AuthorizationDecision>;
}

#[derive(Debug, Default)]
pub struct PermitAllAuthorizationPolicy;

impl AuthorizationPolicy for PermitAllAuthorizationPolicy {
    fn authorize(&self, request: &AuthorizationRequest) -> UnderlayResult<AuthorizationDecision> {
        Ok(AuthorizationDecision {
            operator_id: request.operator_id.clone(),
            role: RbacRole::Admin,
            action: request.action.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub struct StaticAuthorizationPolicy {
    roles_by_operator: BTreeMap<String, BTreeSet<RbacRole>>,
}

impl StaticAuthorizationPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_role(mut self, operator_id: impl Into<String>, role: RbacRole) -> Self {
        self.roles_by_operator
            .entry(operator_id.into())
            .or_default()
            .insert(role);
        self
    }
}

impl AuthorizationPolicy for StaticAuthorizationPolicy {
    fn authorize(&self, request: &AuthorizationRequest) -> UnderlayResult<AuthorizationDecision> {
        let roles = self.roles_by_operator.get(&request.operator_id).ok_or_else(|| {
            UnderlayError::AuthorizationDenied(format!(
                "operator {} has no assigned roles",
                request.operator_id
            ))
        })?;

        roles
            .iter()
            .find(|role| role_allows_action(role, &request.action))
            .cloned()
            .map(|role| AuthorizationDecision {
                operator_id: request.operator_id.clone(),
                role,
                action: request.action.clone(),
            })
            .ok_or_else(|| {
                UnderlayError::AuthorizationDenied(format!(
                    "operator {} is not authorized for {:?}",
                    request.operator_id, request.action
                ))
            })
    }
}

fn role_allows_action(role: &RbacRole, action: &AdminAction) -> bool {
    match action {
        AdminAction::ListOperationSummaries
        | AdminAction::ListAlerts
        | AdminAction::ListInDoubtTransactions
        | AdminAction::GetProductStatusBundle
        | AdminAction::GetWorkerReloadStatus => true,
        AdminAction::AcknowledgeAlert => {
            matches!(
                role,
                RbacRole::Operator | RbacRole::BreakGlassOperator | RbacRole::Admin
            )
        }
        AdminAction::ResolveAlert | AdminAction::SuppressAlert => {
            matches!(role, RbacRole::BreakGlassOperator | RbacRole::Admin)
        }
        AdminAction::ExpireAlert => matches!(role, RbacRole::Admin),
        AdminAction::ForceResolveTransaction => {
            matches!(role, RbacRole::BreakGlassOperator | RbacRole::Admin)
        }
        AdminAction::ForceUnlockSession => false,
        AdminAction::ChangeRetentionPolicy | AdminAction::ChangeDaemonSchedule => {
            matches!(role, RbacRole::Admin)
        }
        AdminAction::ExportAuditHistory => matches!(role, RbacRole::Admin | RbacRole::Auditor),
    }
}
