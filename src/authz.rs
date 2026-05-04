use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{UnderlayError, UnderlayResult};

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
    operators: BTreeSet<String>,
}

impl StaticAuthorizationPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_operator(mut self, operator_id: impl Into<String>) -> Self {
        self.operators.insert(operator_id.into());
        self
    }
}

impl AuthorizationPolicy for StaticAuthorizationPolicy {
    fn authorize(&self, request: &AuthorizationRequest) -> UnderlayResult<AuthorizationDecision> {
        if !self.operators.contains(&request.operator_id) {
            return Err(UnderlayError::AuthorizationDenied(format!(
                "operator {} is not registered for local admin operations",
                request.operator_id
            )));
        }
        Ok(AuthorizationDecision {
            operator_id: request.operator_id.clone(),
            action: request.action.clone(),
        })
    }
}
