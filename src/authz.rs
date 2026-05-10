use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

impl AdminAction {
    pub fn all_actions() -> BTreeSet<Self> {
        [
            Self::ListOperationSummaries,
            Self::ListAlerts,
            Self::ListInDoubtTransactions,
            Self::AcknowledgeAlert,
            Self::ResolveAlert,
            Self::SuppressAlert,
            Self::ExpireAlert,
            Self::ForceResolveTransaction,
            Self::ForceUnlockSession,
            Self::ChangeRetentionPolicy,
            Self::ChangeDaemonSchedule,
            Self::GetProductStatusBundle,
            Self::GetWorkerReloadStatus,
            Self::ExportAuditHistory,
        ]
        .into_iter()
        .collect()
    }
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
            action: request.action.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub struct StaticAuthorizationPolicy {
    operators: BTreeMap<String, BTreeSet<AdminAction>>,
}

impl StaticAuthorizationPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_operator(mut self, operator_id: impl Into<String>) -> Self {
        self.operators
            .insert(operator_id.into(), AdminAction::all_actions());
        self
    }

    pub fn with_allowed_actions<I>(
        mut self,
        operator_id: impl Into<String>,
        actions: I,
    ) -> Self
    where
        I: IntoIterator<Item = AdminAction>,
    {
        self.operators
            .insert(operator_id.into(), actions.into_iter().collect());
        self
    }
}

impl AuthorizationPolicy for StaticAuthorizationPolicy {
    fn authorize(&self, request: &AuthorizationRequest) -> UnderlayResult<AuthorizationDecision> {
        let Some(allowed_actions) = self.operators.get(&request.operator_id) else {
            return Err(UnderlayError::AuthorizationDenied(format!(
                "operator {} is not registered for local admin operations",
                request.operator_id
            )));
        };
        if !allowed_actions.contains(&request.action) {
            return Err(UnderlayError::AuthorizationDenied(format!(
                "operator {} is not authorized for {:?}",
                request.operator_id, request.action
            )));
        }
        Ok(AuthorizationDecision {
            operator_id: request.operator_id.clone(),
            action: request.action.clone(),
        })
    }
}
