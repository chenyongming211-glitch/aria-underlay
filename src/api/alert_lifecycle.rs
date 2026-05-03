use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::authz::{AdminAction, AuthorizationPolicy, AuthorizationRequest};
use crate::telemetry::{
    OperationAlertLifecycleRecord, OperationAlertLifecycleStatus,
    OperationAlertLifecycleStore, OperationAlertLifecycleTransition, ProductAuditRecord,
    ProductAuditStore,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertLifecycleTransitionRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub dedupe_key: String,
    pub operator: String,
    pub reason: String,
    pub target_status: OperationAlertLifecycleStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertLifecycleTransitionResponse {
    pub record: OperationAlertLifecycleRecord,
}

#[derive(Debug, Clone)]
pub struct AlertLifecycleManager {
    authorization_policy: Arc<dyn AuthorizationPolicy>,
    product_audit_store: Arc<dyn ProductAuditStore>,
    lifecycle_store: Arc<dyn OperationAlertLifecycleStore>,
}

impl AlertLifecycleManager {
    pub fn new(
        authorization_policy: Arc<dyn AuthorizationPolicy>,
        product_audit_store: Arc<dyn ProductAuditStore>,
        lifecycle_store: Arc<dyn OperationAlertLifecycleStore>,
    ) -> Self {
        Self {
            authorization_policy,
            product_audit_store,
            lifecycle_store,
        }
    }

    pub fn transition(
        &self,
        request: AlertLifecycleTransitionRequest,
    ) -> UnderlayResult<AlertLifecycleTransitionResponse> {
        validate_transition_request(&request)?;
        let trace_id = request
            .trace_id
            .clone()
            .unwrap_or_else(|| request.request_id.clone());
        let action = action_for_status(&request.target_status)?;
        let decision = self.authorization_policy.authorize(&AuthorizationRequest::new(
            request.request_id.clone(),
            trace_id.clone(),
            request.operator.clone(),
            action,
        ))?;

        self.product_audit_store.append(
            ProductAuditRecord::alert_lifecycle_transition(
                request.request_id.clone(),
                trace_id.clone(),
                request.dedupe_key.clone(),
                request.target_status.clone(),
                request.operator.clone(),
                decision.role.clone(),
                request.reason.clone(),
            ),
        )?;

        let record = self.lifecycle_store.transition(OperationAlertLifecycleTransition {
            dedupe_key: request.dedupe_key,
            status: request.target_status,
            operator_id: request.operator,
            role: Some(decision.role),
            reason: Some(request.reason),
            request_id: request.request_id,
            trace_id,
        })?;
        Ok(AlertLifecycleTransitionResponse { record })
    }
}

fn validate_transition_request(request: &AlertLifecycleTransitionRequest) -> UnderlayResult<()> {
    ensure_non_empty("request_id", &request.request_id)?;
    ensure_non_empty("dedupe_key", &request.dedupe_key)?;
    ensure_non_empty("operator", &request.operator)?;
    ensure_non_empty("reason", &request.reason)?;
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "alert lifecycle {field} must not be empty"
        )));
    }
    Ok(())
}

fn action_for_status(status: &OperationAlertLifecycleStatus) -> UnderlayResult<AdminAction> {
    match status {
        OperationAlertLifecycleStatus::Open => Err(UnderlayError::InvalidIntent(
            "alert lifecycle cannot manually transition to Open".into(),
        )),
        OperationAlertLifecycleStatus::Acknowledged => Ok(AdminAction::AcknowledgeAlert),
        OperationAlertLifecycleStatus::Resolved => Ok(AdminAction::ResolveAlert),
        OperationAlertLifecycleStatus::Suppressed => Ok(AdminAction::SuppressAlert),
        OperationAlertLifecycleStatus::Expired => Ok(AdminAction::ExpireAlert),
    }
}
