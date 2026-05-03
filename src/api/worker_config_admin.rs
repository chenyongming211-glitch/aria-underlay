use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::authz::{AdminAction, AuthorizationPolicy, AuthorizationRequest};
use crate::telemetry::{
    OperationSummaryRetentionPolicy, ProductAuditRecord, ProductAuditStore,
};
use crate::worker::daemon::{UnderlayWorkerDaemonConfig, WorkerScheduleConfig};
use crate::worker::gc::RetentionPolicy;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerScheduleTarget {
    OperationSummaryRetention,
    OperationAlert,
    JournalGc,
    DriftAudit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSummaryRetentionRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub config_path: PathBuf,
    pub operator: String,
    pub reason: String,
    pub retention: OperationSummaryRetentionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeJournalGcRetentionRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub config_path: PathBuf,
    pub operator: String,
    pub reason: String,
    pub retention: RetentionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeWorkerScheduleRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub config_path: PathBuf,
    pub operator: String,
    pub reason: String,
    pub target: WorkerScheduleTarget,
    pub schedule: WorkerScheduleConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerConfigAdminResponse {
    pub config_path: PathBuf,
    pub target: String,
    pub changed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerConfigAdminManager {
    authorization_policy: Arc<dyn AuthorizationPolicy>,
    product_audit_store: Arc<dyn ProductAuditStore>,
}

impl WorkerConfigAdminManager {
    pub fn new(
        authorization_policy: Arc<dyn AuthorizationPolicy>,
        product_audit_store: Arc<dyn ProductAuditStore>,
    ) -> Self {
        Self {
            authorization_policy,
            product_audit_store,
        }
    }

    pub fn change_summary_retention(
        &self,
        request: ChangeSummaryRetentionRequest,
    ) -> UnderlayResult<WorkerConfigAdminResponse> {
        validate_base_request(&request.request_id, &request.operator, &request.reason)?;
        request.retention.validate()?;
        let mut config = UnderlayWorkerDaemonConfig::from_path(&request.config_path)?;
        let Some(operation_summary) = config.operation_summary.as_mut() else {
            return Err(missing_config_section("operation_summary"));
        };

        let trace_id = trace_id_or_request_id(&request.trace_id, &request.request_id);
        let decision = self.authorize(
            &request.request_id,
            &trace_id,
            &request.operator,
            AdminAction::ChangeRetentionPolicy,
        )?;
        let target = "operation_summary";
        let mut fields = base_fields(&request.config_path, target);
        fields.extend(summary_retention_fields(&request.retention));
        self.product_audit_store
            .append(ProductAuditRecord::worker_config_change_requested(
                request.request_id,
                trace_id,
                "daemon.retention_change_requested",
                target,
                request.operator,
                decision.role,
                request.reason,
                fields,
            ))
            .map_err(product_audit_error)?;

        operation_summary.retention = request.retention;
        config.write_to_path(&request.config_path)?;
        Ok(response(request.config_path, target))
    }

    pub fn change_journal_gc_retention(
        &self,
        request: ChangeJournalGcRetentionRequest,
    ) -> UnderlayResult<WorkerConfigAdminResponse> {
        validate_base_request(&request.request_id, &request.operator, &request.reason)?;
        request.retention.validate()?;
        let mut config = UnderlayWorkerDaemonConfig::from_path(&request.config_path)?;
        let Some(journal_gc) = config.journal_gc.as_mut() else {
            return Err(missing_config_section("journal_gc"));
        };

        let trace_id = trace_id_or_request_id(&request.trace_id, &request.request_id);
        let decision = self.authorize(
            &request.request_id,
            &trace_id,
            &request.operator,
            AdminAction::ChangeRetentionPolicy,
        )?;
        let target = "journal_gc";
        let mut fields = base_fields(&request.config_path, target);
        fields.extend(journal_gc_retention_fields(&request.retention));
        self.product_audit_store
            .append(ProductAuditRecord::worker_config_change_requested(
                request.request_id,
                trace_id,
                "daemon.retention_change_requested",
                target,
                request.operator,
                decision.role,
                request.reason,
                fields,
            ))
            .map_err(product_audit_error)?;

        journal_gc.retention = request.retention;
        config.write_to_path(&request.config_path)?;
        Ok(response(request.config_path, target))
    }

    pub fn change_worker_schedule(
        &self,
        request: ChangeWorkerScheduleRequest,
    ) -> UnderlayResult<WorkerConfigAdminResponse> {
        validate_base_request(&request.request_id, &request.operator, &request.reason)?;
        validate_schedule(request.schedule)?;
        let mut config = UnderlayWorkerDaemonConfig::from_path(&request.config_path)?;
        let target = request.target.label();
        schedule_slot(&mut config, &request.target)?;

        let trace_id = trace_id_or_request_id(&request.trace_id, &request.request_id);
        let decision = self.authorize(
            &request.request_id,
            &trace_id,
            &request.operator,
            AdminAction::ChangeDaemonSchedule,
        )?;
        let mut fields = base_fields(&request.config_path, target);
        fields.insert(
            "interval_secs".into(),
            request.schedule.interval_secs.to_string(),
        );
        fields.insert(
            "run_immediately".into(),
            request.schedule.run_immediately.to_string(),
        );
        self.product_audit_store
            .append(ProductAuditRecord::worker_config_change_requested(
                request.request_id,
                trace_id,
                "daemon.schedule_change_requested",
                target,
                request.operator,
                decision.role,
                request.reason,
                fields,
            ))
            .map_err(product_audit_error)?;

        *schedule_slot(&mut config, &request.target)? = request.schedule;
        config.write_to_path(&request.config_path)?;
        Ok(response(request.config_path, target))
    }

    fn authorize(
        &self,
        request_id: &str,
        trace_id: &str,
        operator: &str,
        action: AdminAction,
    ) -> UnderlayResult<crate::authz::AuthorizationDecision> {
        self.authorization_policy.authorize(&AuthorizationRequest::new(
            request_id.to_string(),
            trace_id.to_string(),
            operator.to_string(),
            action,
        ))
    }
}

impl WorkerScheduleTarget {
    pub fn label(&self) -> &'static str {
        match self {
            WorkerScheduleTarget::OperationSummaryRetention => "operation_summary_retention",
            WorkerScheduleTarget::OperationAlert => "operation_alert",
            WorkerScheduleTarget::JournalGc => "journal_gc",
            WorkerScheduleTarget::DriftAudit => "drift_audit",
        }
    }
}

fn schedule_slot<'a>(
    config: &'a mut UnderlayWorkerDaemonConfig,
    target: &WorkerScheduleTarget,
) -> UnderlayResult<&'a mut WorkerScheduleConfig> {
    match target {
        WorkerScheduleTarget::OperationSummaryRetention => config
            .operation_summary
            .as_mut()
            .map(|section| &mut section.retention_schedule)
            .ok_or_else(|| missing_config_section("operation_summary")),
        WorkerScheduleTarget::OperationAlert => config
            .operation_alert
            .as_mut()
            .map(|section| &mut section.schedule)
            .ok_or_else(|| missing_config_section("operation_alert")),
        WorkerScheduleTarget::JournalGc => config
            .journal_gc
            .as_mut()
            .map(|section| &mut section.schedule)
            .ok_or_else(|| missing_config_section("journal_gc")),
        WorkerScheduleTarget::DriftAudit => config
            .drift_audit
            .as_mut()
            .map(|section| &mut section.schedule)
            .ok_or_else(|| missing_config_section("drift_audit")),
    }
}

fn validate_base_request(request_id: &str, operator: &str, reason: &str) -> UnderlayResult<()> {
    ensure_non_empty("request_id", request_id)?;
    ensure_non_empty("operator", operator)?;
    ensure_non_empty("reason", reason)?;
    Ok(())
}

fn validate_schedule(schedule: WorkerScheduleConfig) -> UnderlayResult<()> {
    if schedule.interval_secs == 0 {
        return Err(UnderlayError::InvalidIntent(
            "worker schedule interval_secs must be greater than zero".into(),
        ));
    }
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "worker config admin {field} must not be empty"
        )));
    }
    Ok(())
}

fn missing_config_section(section: &str) -> UnderlayError {
    UnderlayError::InvalidIntent(format!("worker daemon config missing {section} section"))
}

fn trace_id_or_request_id(trace_id: &Option<String>, request_id: &str) -> String {
    trace_id.clone().unwrap_or_else(|| request_id.to_string())
}

fn base_fields(config_path: &std::path::Path, target: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    fields.insert("config_path".into(), config_path.display().to_string());
    fields.insert("target".into(), target.into());
    fields
}

fn summary_retention_fields(
    retention: &OperationSummaryRetentionPolicy,
) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    fields.insert(
        "max_records".into(),
        retention
            .max_records
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".into()),
    );
    fields.insert(
        "max_bytes".into(),
        retention
            .max_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".into()),
    );
    fields.insert(
        "max_rotated_files".into(),
        retention.max_rotated_files.to_string(),
    );
    fields
}

fn journal_gc_retention_fields(retention: &RetentionPolicy) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    fields.insert(
        "committed_journal_retention_days".into(),
        retention.committed_journal_retention_days.to_string(),
    );
    fields.insert(
        "rolled_back_journal_retention_days".into(),
        retention.rolled_back_journal_retention_days.to_string(),
    );
    fields.insert(
        "failed_journal_retention_days".into(),
        retention.failed_journal_retention_days.to_string(),
    );
    fields.insert(
        "rollback_artifact_retention_days".into(),
        retention.rollback_artifact_retention_days.to_string(),
    );
    fields.insert(
        "max_artifacts_per_device".into(),
        retention.max_artifacts_per_device.to_string(),
    );
    fields
}

fn response(config_path: PathBuf, target: &str) -> WorkerConfigAdminResponse {
    WorkerConfigAdminResponse {
        config_path,
        target: target.into(),
        changed: true,
        warnings: Vec::new(),
    }
}

fn product_audit_error(error: UnderlayError) -> UnderlayError {
    match error {
        UnderlayError::ProductAuditWriteFailed(_) => error,
        other => UnderlayError::ProductAuditWriteFailed(other.to_string()),
    }
}
