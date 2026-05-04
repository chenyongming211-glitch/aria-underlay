use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::operations::{
    ListOperationSummariesRequest, ListOperationSummariesResponse,
};
use crate::api::product_ops::{
    ExportProductAuditRequest, ExportProductAuditResponse,
    ProductGetWorkerReloadStatusRequest,
    ProductChangeJournalGcRetentionRequest, ProductChangeSummaryRetentionRequest,
    ProductChangeWorkerScheduleRequest, ProductOperatorContext, ProductOpsManager,
};
use crate::api::worker_config_admin::WorkerConfigAdminResponse;
use crate::authz::{RbacRole, StaticAuthorizationPolicy};
use crate::telemetry::{OperationSummaryStore, ProductAuditStore};
use crate::worker::daemon::WorkerReloadCheckpoint;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductApiRequest<T> {
    pub request_id: String,
    pub trace_id: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductApiResponse<T> {
    pub request_id: String,
    pub trace_id: String,
    pub operator_id: String,
    pub role: RbacRole,
    pub body: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductApiRequestMetadata {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSession {
    pub operator_id: String,
    pub role: RbacRole,
}

pub trait ProductSessionExtractor: std::fmt::Debug + Send + Sync {
    fn extract(&self, metadata: &ProductApiRequestMetadata) -> UnderlayResult<ProductSession>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderProductSessionExtractor {
    operator_header: String,
    role_header: String,
}

impl Default for HeaderProductSessionExtractor {
    fn default() -> Self {
        Self {
            operator_header: "x-aria-operator-id".into(),
            role_header: "x-aria-role".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProductOpsApi {
    session_extractor: Arc<dyn ProductSessionExtractor>,
    operation_summary_store: Arc<dyn OperationSummaryStore>,
    product_audit_store: Arc<dyn ProductAuditStore>,
}

impl ProductOpsApi {
    pub fn new(
        session_extractor: Arc<dyn ProductSessionExtractor>,
        operation_summary_store: Arc<dyn OperationSummaryStore>,
        product_audit_store: Arc<dyn ProductAuditStore>,
    ) -> Self {
        Self {
            session_extractor,
            operation_summary_store,
            product_audit_store,
        }
    }

    pub fn list_operation_summaries(
        &self,
        request: ProductApiRequest<ListOperationSummariesRequest>,
    ) -> UnderlayResult<ProductApiResponse<ListOperationSummariesResponse>> {
        let metadata = request.metadata();
        let session = self.session_extractor.extract(&metadata)?;
        let trace_id = trace_id_or_request_id(&metadata);
        let body = self
            .manager_for_session(&session)
            .list_operation_summaries(
                operator_context(&metadata, &session),
                request.body,
            )?;
        Ok(api_response(metadata, session, trace_id, body))
    }

    pub fn export_product_audit(
        &self,
        request: ProductApiRequest<ExportProductAuditRequest>,
    ) -> UnderlayResult<ProductApiResponse<ExportProductAuditResponse>> {
        let metadata = request.metadata();
        let session = self.session_extractor.extract(&metadata)?;
        let trace_id = trace_id_or_request_id(&metadata);
        let body = self
            .manager_for_session(&session)
            .export_product_audit(operator_context(&metadata, &session), request.body)?;
        Ok(api_response(metadata, session, trace_id, body))
    }

    pub fn change_summary_retention(
        &self,
        request: ProductApiRequest<ProductChangeSummaryRetentionRequest>,
    ) -> UnderlayResult<ProductApiResponse<WorkerConfigAdminResponse>> {
        let metadata = request.metadata();
        let session = self.session_extractor.extract(&metadata)?;
        let trace_id = trace_id_or_request_id(&metadata);
        let body = self
            .manager_for_session(&session)
            .change_summary_retention(operator_context(&metadata, &session), request.body)?;
        Ok(api_response(metadata, session, trace_id, body))
    }

    pub fn change_journal_gc_retention(
        &self,
        request: ProductApiRequest<ProductChangeJournalGcRetentionRequest>,
    ) -> UnderlayResult<ProductApiResponse<WorkerConfigAdminResponse>> {
        let metadata = request.metadata();
        let session = self.session_extractor.extract(&metadata)?;
        let trace_id = trace_id_or_request_id(&metadata);
        let body = self
            .manager_for_session(&session)
            .change_journal_gc_retention(operator_context(&metadata, &session), request.body)?;
        Ok(api_response(metadata, session, trace_id, body))
    }

    pub fn change_worker_schedule(
        &self,
        request: ProductApiRequest<ProductChangeWorkerScheduleRequest>,
    ) -> UnderlayResult<ProductApiResponse<WorkerConfigAdminResponse>> {
        let metadata = request.metadata();
        let session = self.session_extractor.extract(&metadata)?;
        let trace_id = trace_id_or_request_id(&metadata);
        let body = self
            .manager_for_session(&session)
            .change_worker_schedule(operator_context(&metadata, &session), request.body)?;
        Ok(api_response(metadata, session, trace_id, body))
    }

    pub fn get_worker_reload_status(
        &self,
        request: ProductApiRequest<ProductGetWorkerReloadStatusRequest>,
    ) -> UnderlayResult<ProductApiResponse<WorkerReloadCheckpoint>> {
        let metadata = request.metadata();
        let session = self.session_extractor.extract(&metadata)?;
        let trace_id = trace_id_or_request_id(&metadata);
        let body = self
            .manager_for_session(&session)
            .get_worker_reload_status(operator_context(&metadata, &session), request.body)?;
        Ok(api_response(metadata, session, trace_id, body))
    }

    fn manager_for_session(&self, session: &ProductSession) -> ProductOpsManager {
        ProductOpsManager::new(
            Arc::new(
                StaticAuthorizationPolicy::new()
                    .with_role(session.operator_id.clone(), session.role.clone()),
            ),
            self.operation_summary_store.clone(),
            self.product_audit_store.clone(),
        )
    }
}

impl<T> ProductApiRequest<T> {
    fn metadata(&self) -> ProductApiRequestMetadata {
        ProductApiRequestMetadata {
            request_id: self.request_id.clone(),
            trace_id: self.trace_id.clone(),
            headers: self.headers.clone(),
        }
    }
}

impl HeaderProductSessionExtractor {
    pub fn new(
        operator_header: impl Into<String>,
        role_header: impl Into<String>,
    ) -> Self {
        Self {
            operator_header: operator_header.into(),
            role_header: role_header.into(),
        }
    }
}

impl ProductSessionExtractor for HeaderProductSessionExtractor {
    fn extract(&self, metadata: &ProductApiRequestMetadata) -> UnderlayResult<ProductSession> {
        validate_non_empty("product api request_id", &metadata.request_id)?;
        let operator_id = header_value(&metadata.headers, &self.operator_header)
            .ok_or_else(|| missing_header_error(&self.operator_header))?;
        let role = header_value(&metadata.headers, &self.role_header)
            .ok_or_else(|| missing_header_error(&self.role_header))
            .and_then(|value| parse_role(&self.role_header, &value))?;
        Ok(ProductSession { operator_id, role })
    }
}

fn operator_context(
    metadata: &ProductApiRequestMetadata,
    session: &ProductSession,
) -> ProductOperatorContext {
    ProductOperatorContext {
        request_id: metadata.request_id.clone(),
        trace_id: metadata.trace_id.clone(),
        operator: session.operator_id.clone(),
    }
}

fn api_response<T>(
    metadata: ProductApiRequestMetadata,
    session: ProductSession,
    trace_id: String,
    body: T,
) -> ProductApiResponse<T> {
    ProductApiResponse {
        request_id: metadata.request_id,
        trace_id,
        operator_id: session.operator_id,
        role: session.role,
        body,
    }
}

fn trace_id_or_request_id(metadata: &ProductApiRequestMetadata) -> String {
    metadata
        .trace_id
        .clone()
        .unwrap_or_else(|| metadata.request_id.clone())
}

fn header_value(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_role(header: &str, value: &str) -> UnderlayResult<RbacRole> {
    match value {
        "Viewer" | "viewer" => Ok(RbacRole::Viewer),
        "Operator" | "operator" => Ok(RbacRole::Operator),
        "BreakGlassOperator" | "break-glass-operator" | "break_glass_operator" => {
            Ok(RbacRole::BreakGlassOperator)
        }
        "Admin" | "admin" => Ok(RbacRole::Admin),
        "Auditor" | "auditor" => Ok(RbacRole::Auditor),
        _ => Err(UnderlayError::InvalidIntent(format!(
            "{header} must be Viewer, Operator, BreakGlassOperator, Admin, or Auditor"
        ))),
    }
}

fn missing_header_error(header: &str) -> UnderlayError {
    UnderlayError::InvalidIntent(format!("missing required product API header {header}"))
}

fn validate_non_empty(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}
