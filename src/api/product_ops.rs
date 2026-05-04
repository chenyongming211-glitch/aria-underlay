use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::operations::{
    ListOperationSummariesRequest, ListOperationSummariesResponse, OperationSummaryOverview,
};
use crate::api::worker_config_admin::{
    ChangeJournalGcRetentionRequest, ChangeSummaryRetentionRequest,
    ChangeWorkerScheduleRequest, WorkerConfigAdminManager, WorkerConfigAdminResponse,
    WorkerScheduleTarget,
};
use crate::authz::{
    AdminAction, AuthorizationDecision, AuthorizationPolicy, AuthorizationRequest,
};
use crate::telemetry::{
    OperationSummary, OperationSummaryRetentionPolicy, OperationSummaryStore,
    ProductAuditRecord, ProductAuditStore,
};
use crate::worker::daemon::{WorkerReloadCheckpoint, WorkerScheduleConfig};
use crate::worker::gc::RetentionPolicy;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductOperatorContext {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub operator: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProductAuditRequest {
    pub reason: String,
    pub action: Option<String>,
    pub result: Option<String>,
    pub operator_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductAuditExportOverview {
    pub matched_records: usize,
    pub returned_records: usize,
    pub attention_required: usize,
    pub by_action: BTreeMap<String, usize>,
    pub by_result: BTreeMap<String, usize>,
    pub by_operator: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProductAuditResponse {
    pub records: Vec<ProductAuditRecord>,
    pub overview: ProductAuditExportOverview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductChangeSummaryRetentionRequest {
    pub config_path: std::path::PathBuf,
    pub reason: String,
    pub retention: OperationSummaryRetentionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductChangeJournalGcRetentionRequest {
    pub config_path: std::path::PathBuf,
    pub reason: String,
    pub retention: RetentionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductChangeWorkerScheduleRequest {
    pub config_path: std::path::PathBuf,
    pub reason: String,
    pub target: WorkerScheduleTarget,
    pub schedule: WorkerScheduleConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductGetWorkerReloadStatusRequest {
    pub checkpoint_path: std::path::PathBuf,
}

impl Default for ProductChangeSummaryRetentionRequest {
    fn default() -> Self {
        Self {
            config_path: std::path::PathBuf::new(),
            reason: String::new(),
            retention: OperationSummaryRetentionPolicy::default(),
        }
    }
}

impl Default for ProductChangeJournalGcRetentionRequest {
    fn default() -> Self {
        Self {
            config_path: std::path::PathBuf::new(),
            reason: String::new(),
            retention: RetentionPolicy::default(),
        }
    }
}

impl Default for ProductChangeWorkerScheduleRequest {
    fn default() -> Self {
        Self {
            config_path: std::path::PathBuf::new(),
            reason: String::new(),
            target: WorkerScheduleTarget::OperationSummaryRetention,
            schedule: WorkerScheduleConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProductOpsManager {
    authorization_policy: Arc<dyn AuthorizationPolicy>,
    operation_summary_store: Arc<dyn OperationSummaryStore>,
    product_audit_store: Arc<dyn ProductAuditStore>,
}

impl ProductOpsManager {
    pub fn new(
        authorization_policy: Arc<dyn AuthorizationPolicy>,
        operation_summary_store: Arc<dyn OperationSummaryStore>,
        product_audit_store: Arc<dyn ProductAuditStore>,
    ) -> Self {
        Self {
            authorization_policy,
            operation_summary_store,
            product_audit_store,
        }
    }

    pub fn list_operation_summaries(
        &self,
        context: ProductOperatorContext,
        request: ListOperationSummariesRequest,
    ) -> UnderlayResult<ListOperationSummariesResponse> {
        let limit = request.limit;
        let (_trace_id, _decision) = self.authorize_context(
            &context,
            AdminAction::ListOperationSummaries,
            "product ops list operation summaries",
        )?;
        let summaries = filtered_operation_summaries(
            if request.attention_required_only {
                self.operation_summary_store.list_attention_required()?
            } else {
                self.operation_summary_store.list()?
            },
            request,
        );
        let returned_summaries = limit
            .map(|limit| limit_newest(summaries.clone(), limit))
            .unwrap_or_else(|| summaries.clone());
        let overview = OperationSummaryOverview::from_summaries(
            &summaries,
            returned_summaries.len(),
        );
        Ok(ListOperationSummariesResponse {
            summaries: returned_summaries,
            overview,
        })
    }

    pub fn export_product_audit(
        &self,
        context: ProductOperatorContext,
        request: ExportProductAuditRequest,
    ) -> UnderlayResult<ExportProductAuditResponse> {
        validate_non_empty("product audit export reason", &request.reason)?;
        let (trace_id, decision) = self.authorize_context(
            &context,
            AdminAction::ExportAuditHistory,
            "product audit export",
        )?;
        self.product_audit_store
            .append(ProductAuditRecord::product_audit_export_requested(
                context.request_id.clone(),
                trace_id,
                decision.operator_id,
                decision.role,
                request.reason.clone(),
                export_filter_fields(&request),
            ))
            .map_err(product_audit_error)?;

        let records = filtered_audit_records(self.product_audit_store.list()?, &request);
        let returned_records = request
            .limit
            .map(|limit| limit_newest(records.clone(), limit))
            .unwrap_or_else(|| records.clone());
        let overview =
            ProductAuditExportOverview::from_records(&records, returned_records.len());
        Ok(ExportProductAuditResponse {
            records: returned_records,
            overview,
        })
    }

    pub fn change_summary_retention(
        &self,
        context: ProductOperatorContext,
        request: ProductChangeSummaryRetentionRequest,
    ) -> UnderlayResult<WorkerConfigAdminResponse> {
        self.worker_config_admin()
            .change_summary_retention(ChangeSummaryRetentionRequest {
                request_id: context.request_id,
                trace_id: context.trace_id,
                config_path: request.config_path,
                operator: context.operator,
                reason: request.reason,
                retention: request.retention,
            })
    }

    pub fn change_journal_gc_retention(
        &self,
        context: ProductOperatorContext,
        request: ProductChangeJournalGcRetentionRequest,
    ) -> UnderlayResult<WorkerConfigAdminResponse> {
        self.worker_config_admin()
            .change_journal_gc_retention(ChangeJournalGcRetentionRequest {
                request_id: context.request_id,
                trace_id: context.trace_id,
                config_path: request.config_path,
                operator: context.operator,
                reason: request.reason,
                retention: request.retention,
            })
    }

    pub fn change_worker_schedule(
        &self,
        context: ProductOperatorContext,
        request: ProductChangeWorkerScheduleRequest,
    ) -> UnderlayResult<WorkerConfigAdminResponse> {
        self.worker_config_admin()
            .change_worker_schedule(ChangeWorkerScheduleRequest {
                request_id: context.request_id,
                trace_id: context.trace_id,
                config_path: request.config_path,
                operator: context.operator,
                reason: request.reason,
                target: request.target,
                schedule: request.schedule,
            })
    }

    pub fn get_worker_reload_status(
        &self,
        context: ProductOperatorContext,
        request: ProductGetWorkerReloadStatusRequest,
    ) -> UnderlayResult<WorkerReloadCheckpoint> {
        validate_path("worker reload checkpoint path", &request.checkpoint_path)?;
        let (_trace_id, _decision) = self.authorize_context(
            &context,
            AdminAction::GetWorkerReloadStatus,
            "product ops get worker reload status",
        )?;
        WorkerReloadCheckpoint::from_path(request.checkpoint_path)
    }

    fn worker_config_admin(&self) -> WorkerConfigAdminManager {
        WorkerConfigAdminManager::new(
            self.authorization_policy.clone(),
            self.product_audit_store.clone(),
        )
    }

    fn authorize_context(
        &self,
        context: &ProductOperatorContext,
        action: AdminAction,
        label: &str,
    ) -> UnderlayResult<(String, AuthorizationDecision)> {
        validate_non_empty(&format!("{label} request_id"), &context.request_id)?;
        validate_non_empty(&format!("{label} operator"), &context.operator)?;
        let trace_id = context
            .trace_id
            .clone()
            .unwrap_or_else(|| context.request_id.clone());
        let decision = self.authorization_policy.authorize(&AuthorizationRequest::new(
            context.request_id.clone(),
            trace_id.clone(),
            context.operator.clone(),
            action,
        ))?;
        Ok((trace_id, decision))
    }
}

impl ProductAuditExportOverview {
    pub fn from_records(records: &[ProductAuditRecord], returned_records: usize) -> Self {
        let mut overview = Self {
            matched_records: records.len(),
            returned_records,
            ..Default::default()
        };

        for record in records {
            if record.attention_required {
                overview.attention_required += 1;
            }
            increment(&mut overview.by_action, &record.action);
            increment(&mut overview.by_result, &record.result);
            if let Some(operator_id) = &record.operator_id {
                increment(&mut overview.by_operator, operator_id);
            }
        }

        overview
    }
}

fn filtered_operation_summaries(
    mut summaries: Vec<OperationSummary>,
    request: ListOperationSummariesRequest,
) -> Vec<OperationSummary> {
    if let Some(action) = request.action {
        summaries.retain(|summary| summary.action == action);
    }
    if let Some(result) = request.result {
        summaries.retain(|summary| summary.result == result);
    }
    if let Some(device_id) = request.device_id {
        summaries.retain(|summary| summary.device_id.as_ref() == Some(&device_id));
    }
    if let Some(tx_id) = request.tx_id {
        summaries.retain(|summary| summary.tx_id.as_deref() == Some(tx_id.as_str()));
    }
    summaries
}

fn filtered_audit_records(
    mut records: Vec<ProductAuditRecord>,
    request: &ExportProductAuditRequest,
) -> Vec<ProductAuditRecord> {
    if let Some(action) = &request.action {
        records.retain(|record| record.action == *action);
    }
    if let Some(result) = &request.result {
        records.retain(|record| record.result == *result);
    }
    if let Some(operator_id) = &request.operator_id {
        records.retain(|record| record.operator_id.as_deref() == Some(operator_id.as_str()));
    }
    records
}

fn export_filter_fields(request: &ExportProductAuditRequest) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    if let Some(action) = &request.action {
        fields.insert("filter_action".into(), action.clone());
    }
    if let Some(result) = &request.result {
        fields.insert("filter_result".into(), result.clone());
    }
    if let Some(operator_id) = &request.operator_id {
        fields.insert("filter_operator_id".into(), operator_id.clone());
    }
    if let Some(limit) = request.limit {
        fields.insert("limit".into(), limit.to_string());
    }
    fields
}

fn limit_newest<T: Clone>(items: Vec<T>, limit: usize) -> Vec<T> {
    if items.len() <= limit {
        return items;
    }
    let mut returned = items.into_iter().rev().take(limit).collect::<Vec<_>>();
    returned.reverse();
    returned
}

fn validate_non_empty(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_path(field: &str, value: &std::path::Path) -> UnderlayResult<()> {
    if value.as_os_str().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn product_audit_error(error: UnderlayError) -> UnderlayError {
    match error {
        UnderlayError::ProductAuditWriteFailed(_) => error,
        other => UnderlayError::ProductAuditWriteFailed(other.to_string()),
    }
}

fn increment(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}
