use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::Arc;

use serde::Serialize;

use crate::api::alert_lifecycle::{AlertLifecycleManager, AlertLifecycleTransitionRequest};
use crate::api::force_resolve::ForceResolveTransactionRequest;
use crate::api::operations::ListOperationSummariesRequest;
use crate::api::transactions::ListInDoubtTransactionsRequest;
use crate::api::worker_config_admin::{
    ChangeJournalGcRetentionRequest, ChangeSummaryRetentionRequest, ChangeWorkerScheduleRequest,
    WorkerConfigAdminManager, WorkerScheduleTarget,
};
use crate::api::{AriaUnderlayService, UnderlayService};
use crate::authz::{RbacRole, StaticAuthorizationPolicy};
use crate::device::DeviceInventory;
use crate::model::DeviceId;
use crate::telemetry::{
    JsonFileOperationAlertLifecycleStore, JsonFileOperationAlertSink,
    JsonFileOperationSummaryStore, JsonFileProductAuditStore, OperationAlert,
    OperationAlertLifecycleRecord, OperationAlertLifecycleStatus, OperationAlertLifecycleStore,
    OperationAlertSeverity, OperationSummaryRetentionPolicy,
};
use crate::tx::JsonFileTxJournalStore;
use crate::worker::daemon::{WorkerReloadCheckpoint, WorkerScheduleConfig};
use crate::worker::deployment::WorkerDeploymentPreflight;
use crate::worker::gc::RetentionPolicy;

pub async fn run<I>(args: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = String>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Err(invalid_input("missing command").into());
    };

    match command {
        "list-in-doubt" => list_in_doubt(&args[1..]).await,
        "force-resolve" => force_resolve(&args[1..]).await,
        "list-operations" => list_operations(&args[1..]).await,
        "operation-summary" => operation_summary(&args[1..]).await,
        "list-alerts" => list_alerts(&args[1..]).await,
        "alert-summary" => alert_summary(&args[1..]).await,
        "ack-alert" => {
            transition_alert(&args[1..], OperationAlertLifecycleStatus::Acknowledged).await
        }
        "resolve-alert" => {
            transition_alert(&args[1..], OperationAlertLifecycleStatus::Resolved).await
        }
        "suppress-alert" => {
            transition_alert(&args[1..], OperationAlertLifecycleStatus::Suppressed).await
        }
        "expire-alert" => {
            transition_alert(&args[1..], OperationAlertLifecycleStatus::Expired).await
        }
        "set-summary-retention" => set_summary_retention(&args[1..]).await,
        "set-gc-retention" => set_gc_retention(&args[1..]).await,
        "set-worker-schedule" => set_worker_schedule(&args[1..]).await,
        "check-worker-config" => check_worker_config(&args[1..]).await,
        "worker-reload-status" => worker_reload_status(&args[1..]).await,
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        unknown => {
            print_usage();
            Err(invalid_input(format!("unknown command {unknown}")).into())
        }
    }
}

async fn list_in_doubt(args: &[String]) -> Result<(), Box<dyn Error>> {
    let journal_root = required_option(args, "--journal-root")?;
    let device_id = option_value(args, "--device-id").map(DeviceId);
    let service = service_for_journal_root(journal_root, None);
    let response = service
        .list_in_doubt_transactions(ListInDoubtTransactionsRequest { device_id })
        .await?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn force_resolve(args: &[String]) -> Result<(), Box<dyn Error>> {
    let journal_root = required_option(args, "--journal-root")?;
    let operation_summary_path = Some(required_option(args, "--operation-summary-path")?);
    let tx_id = required_option(args, "--tx-id")?.to_string();
    let operator = required_option(args, "--operator")?.to_string();
    let reason = required_option(args, "--reason")?.to_string();
    let request_id = option_value(args, "--request-id")
        .unwrap_or_else(|| format!("force-resolve-{}", uuid::Uuid::new_v4()));
    let trace_id = option_value(args, "--trace-id");
    let service = service_for_journal_root(journal_root, operation_summary_path);
    let response = service
        .force_resolve_transaction(ForceResolveTransactionRequest {
            request_id,
            trace_id,
            tx_id,
            operator,
            reason,
            break_glass_enabled: has_flag(args, "--break-glass"),
        })
        .await?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_operations(args: &[String]) -> Result<(), Box<dyn Error>> {
    let operation_summary_path = required_option(args, "--operation-summary-path")?;
    let service = service_for_operation_summary_path(operation_summary_path);
    let response = service
        .list_operation_summaries(operation_summary_request(args)?)
        .await?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn operation_summary(args: &[String]) -> Result<(), Box<dyn Error>> {
    let operation_summary_path = required_option(args, "--operation-summary-path")?;
    let service = service_for_operation_summary_path(operation_summary_path);
    let response = service
        .list_operation_summaries(operation_summary_request(args)?)
        .await?;

    println!("{}", serde_json::to_string_pretty(&response.overview)?);
    Ok(())
}

async fn list_alerts(args: &[String]) -> Result<(), Box<dyn Error>> {
    let operation_alert_path = required_option(args, "--operation-alert-path")?;
    let alerts = filtered_alerts(args, operation_alert_path)?;
    let lifecycle_records = lifecycle_records(args)?;
    let limit = optional_usize(args, "--limit")?;
    let returned_alerts = limit_alerts(&alerts, &lifecycle_records, limit);
    let response = ListOperationAlertsResponse {
        overview: OperationAlertOverview::from_alerts(
            &alerts,
            returned_alerts.len(),
            &lifecycle_records,
        ),
        alerts: returned_alerts,
    };

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn alert_summary(args: &[String]) -> Result<(), Box<dyn Error>> {
    let operation_alert_path = required_option(args, "--operation-alert-path")?;
    let alerts = filtered_alerts(args, operation_alert_path)?;
    let lifecycle_records = lifecycle_records(args)?;
    let overview = OperationAlertOverview::from_alerts(&alerts, alerts.len(), &lifecycle_records);

    println!("{}", serde_json::to_string_pretty(&overview)?);
    Ok(())
}

async fn transition_alert(
    args: &[String],
    target_status: OperationAlertLifecycleStatus,
) -> Result<(), Box<dyn Error>> {
    let alert_state_path = required_option(args, "--alert-state-path")?;
    let product_audit_path = required_option(args, "--product-audit-path")?;
    let dedupe_key = required_option(args, "--dedupe-key")?;
    let operator = required_option(args, "--operator")?;
    let role = required_role(args, "--role")?;
    let reason = required_option(args, "--reason")?;
    let request_id = option_value(args, "--request-id")
        .unwrap_or_else(|| format!("alert-lifecycle-{}", uuid::Uuid::new_v4()));
    let trace_id = option_value(args, "--trace-id");
    let authorization_policy = StaticAuthorizationPolicy::new().with_role(operator.clone(), role);
    let manager = AlertLifecycleManager::new(
        Arc::new(authorization_policy),
        Arc::new(JsonFileProductAuditStore::new(product_audit_path)),
        Arc::new(JsonFileOperationAlertLifecycleStore::new(alert_state_path)),
    );
    let response = manager.transition(AlertLifecycleTransitionRequest {
        request_id,
        trace_id,
        dedupe_key,
        operator,
        reason,
        target_status,
    })?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn set_summary_retention(args: &[String]) -> Result<(), Box<dyn Error>> {
    let manager = worker_config_admin_manager(args)?;
    let request_id = request_id(args, "set-summary-retention");
    let response = manager.change_summary_retention(ChangeSummaryRetentionRequest {
        request_id,
        trace_id: option_value(args, "--trace-id"),
        config_path: required_option(args, "--worker-config-path")?.into(),
        operator: required_option(args, "--operator")?,
        reason: required_option(args, "--reason")?,
        retention: OperationSummaryRetentionPolicy {
            max_records: optional_usize(args, "--max-records")?,
            max_bytes: optional_u64(args, "--max-bytes")?,
            max_rotated_files: optional_usize(args, "--max-rotated-files")?
                .unwrap_or_else(|| OperationSummaryRetentionPolicy::default().max_rotated_files),
        },
    })?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn set_gc_retention(args: &[String]) -> Result<(), Box<dyn Error>> {
    let manager = worker_config_admin_manager(args)?;
    let request_id = request_id(args, "set-gc-retention");
    let response = manager.change_journal_gc_retention(ChangeJournalGcRetentionRequest {
        request_id,
        trace_id: option_value(args, "--trace-id"),
        config_path: required_option(args, "--worker-config-path")?.into(),
        operator: required_option(args, "--operator")?,
        reason: required_option(args, "--reason")?,
        retention: RetentionPolicy {
            committed_journal_retention_days: required_u32(args, "--committed-days")?,
            rolled_back_journal_retention_days: required_u32(args, "--rolled-back-days")?,
            failed_journal_retention_days: required_u32(args, "--failed-days")?,
            rollback_artifact_retention_days: required_u32(args, "--rollback-artifact-days")?,
            max_artifacts_per_device: required_u32(args, "--max-artifacts-per-device")?,
        },
    })?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn set_worker_schedule(args: &[String]) -> Result<(), Box<dyn Error>> {
    let manager = worker_config_admin_manager(args)?;
    let request_id = request_id(args, "set-worker-schedule");
    let response = manager.change_worker_schedule(ChangeWorkerScheduleRequest {
        request_id,
        trace_id: option_value(args, "--trace-id"),
        config_path: required_option(args, "--worker-config-path")?.into(),
        operator: required_option(args, "--operator")?,
        reason: required_option(args, "--reason")?,
        target: required_worker_schedule_target(args, "--target")?,
        schedule: WorkerScheduleConfig {
            interval_secs: required_u64(args, "--interval-secs")?,
            run_immediately: required_bool(args, "--run-immediately")?,
        },
    })?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn check_worker_config(args: &[String]) -> Result<(), Box<dyn Error>> {
    let worker_config_path = required_option(args, "--worker-config-path")?;
    let report = WorkerDeploymentPreflight::new()
        .strict_paths(has_flag(args, "--strict-paths"))
        .check_config_path(worker_config_path.as_str());

    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.valid {
        Ok(())
    } else {
        Err(invalid_input("worker config preflight failed").into())
    }
}

async fn worker_reload_status(args: &[String]) -> Result<(), Box<dyn Error>> {
    let checkpoint_path = required_option(args, "--checkpoint-path")?;
    let checkpoint = WorkerReloadCheckpoint::from_path(checkpoint_path.as_str())?;

    println!("{}", serde_json::to_string_pretty(&checkpoint)?);
    Ok(())
}

fn worker_config_admin_manager(
    args: &[String],
) -> Result<WorkerConfigAdminManager, Box<dyn Error>> {
    let operator = required_option(args, "--operator")?;
    let role = required_role(args, "--role")?;
    let product_audit_path = required_option(args, "--product-audit-path")?;
    Ok(WorkerConfigAdminManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role(operator, role)),
        Arc::new(JsonFileProductAuditStore::new(product_audit_path)),
    ))
}

fn service_for_journal_root(
    journal_root: String,
    operation_summary_path: Option<String>,
) -> AriaUnderlayService {
    let service = AriaUnderlayService::new_with_journal(
        DeviceInventory::default(),
        Arc::new(JsonFileTxJournalStore::new(journal_root)),
    );
    if let Some(operation_summary_path) = operation_summary_path {
        service.with_operation_summary_store(Arc::new(JsonFileOperationSummaryStore::new(
            operation_summary_path,
        )))
    } else {
        service
    }
}

fn service_for_operation_summary_path(operation_summary_path: String) -> AriaUnderlayService {
    AriaUnderlayService::new(DeviceInventory::default()).with_operation_summary_store(Arc::new(
        JsonFileOperationSummaryStore::new(operation_summary_path),
    ))
}

fn operation_summary_request(args: &[String]) -> Result<ListOperationSummariesRequest, io::Error> {
    Ok(ListOperationSummariesRequest {
        attention_required_only: has_flag(args, "--attention-required"),
        action: option_value(args, "--action"),
        result: option_value(args, "--result"),
        device_id: option_value(args, "--device-id").map(DeviceId),
        tx_id: option_value(args, "--tx-id"),
        limit: optional_usize(args, "--limit")?,
    })
}

fn filtered_alerts(
    args: &[String],
    operation_alert_path: String,
) -> Result<Vec<OperationAlert>, Box<dyn Error>> {
    let severity = optional_alert_severity(args, "--severity")?;
    let alerts = JsonFileOperationAlertSink::new(operation_alert_path)
        .list()?
        .into_iter()
        .filter(|alert| {
            severity
                .as_ref()
                .map(|severity| alert.severity == *severity)
                .unwrap_or(true)
        })
        .collect();
    Ok(alerts)
}

fn lifecycle_records(
    args: &[String],
) -> Result<BTreeMap<String, OperationAlertLifecycleRecord>, Box<dyn Error>> {
    let Some(alert_state_path) = option_value(args, "--alert-state-path") else {
        return Ok(BTreeMap::new());
    };
    let records = JsonFileOperationAlertLifecycleStore::new(alert_state_path)
        .list()?
        .into_iter()
        .map(|record| (record.dedupe_key.clone(), record))
        .collect();
    Ok(records)
}

fn limit_alerts(
    alerts: &[OperationAlert],
    lifecycle_records: &BTreeMap<String, OperationAlertLifecycleRecord>,
    limit: Option<usize>,
) -> Vec<OperationAlertView> {
    alerts
        .iter()
        .take(limit.unwrap_or(alerts.len()))
        .map(|alert| OperationAlertView {
            alert: alert.clone(),
            lifecycle: lifecycle_records.get(&alert.dedupe_key).cloned(),
        })
        .collect()
}

#[derive(Debug, Serialize)]
struct ListOperationAlertsResponse {
    alerts: Vec<OperationAlertView>,
    overview: OperationAlertOverview,
}

#[derive(Debug, Serialize)]
struct OperationAlertView {
    #[serde(flatten)]
    alert: OperationAlert,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle: Option<OperationAlertLifecycleRecord>,
}

#[derive(Debug, Default, Serialize)]
struct OperationAlertOverview {
    matched_alerts: usize,
    returned_alerts: usize,
    critical: usize,
    warning: usize,
    open: usize,
    acknowledged: usize,
    resolved: usize,
    suppressed: usize,
    expired: usize,
    by_action: BTreeMap<String, usize>,
    by_result: BTreeMap<String, usize>,
    by_device: BTreeMap<String, usize>,
}

impl OperationAlertOverview {
    fn from_alerts(
        alerts: &[OperationAlert],
        returned_alerts: usize,
        lifecycle_records: &BTreeMap<String, OperationAlertLifecycleRecord>,
    ) -> Self {
        let mut overview = Self {
            matched_alerts: alerts.len(),
            returned_alerts,
            ..Default::default()
        };

        for alert in alerts {
            match alert.severity {
                OperationAlertSeverity::Critical => overview.critical += 1,
                OperationAlertSeverity::Warning => overview.warning += 1,
            }
            increment(&mut overview.by_action, &alert.action);
            increment(&mut overview.by_result, &alert.result);
            if let Some(device_id) = &alert.device_id {
                increment(&mut overview.by_device, &device_id.0);
            }
            overview.record_lifecycle_status(
                lifecycle_records
                    .get(&alert.dedupe_key)
                    .map(|record| &record.status)
                    .unwrap_or(&OperationAlertLifecycleStatus::Open),
            );
        }

        overview
    }

    fn record_lifecycle_status(&mut self, status: &OperationAlertLifecycleStatus) {
        match status {
            OperationAlertLifecycleStatus::Open => self.open += 1,
            OperationAlertLifecycleStatus::Acknowledged => self.acknowledged += 1,
            OperationAlertLifecycleStatus::Resolved => self.resolved += 1,
            OperationAlertLifecycleStatus::Suppressed => self.suppressed += 1,
            OperationAlertLifecycleStatus::Expired => self.expired += 1,
        }
    }
}

fn increment(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}

fn required_option(args: &[String], name: &str) -> Result<String, io::Error> {
    option_value(args, name).ok_or_else(|| invalid_input(format!("missing required {name}")))
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn optional_usize(args: &[String], name: &str) -> Result<Option<usize>, io::Error> {
    option_value(args, name)
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| invalid_input(format!("{name} must be an unsigned integer")))
        })
        .transpose()
}

fn optional_u64(args: &[String], name: &str) -> Result<Option<u64>, io::Error> {
    option_value(args, name)
        .map(|value| parse_u64(&value, name))
        .transpose()
}

fn required_u64(args: &[String], name: &str) -> Result<u64, io::Error> {
    parse_u64(&required_option(args, name)?, name)
}

fn required_u32(args: &[String], name: &str) -> Result<u32, io::Error> {
    let value = required_u64(args, name)?;
    u32::try_from(value).map_err(|_| invalid_input(format!("{name} must fit in u32")))
}

fn parse_u64(value: &str, name: &str) -> Result<u64, io::Error> {
    value
        .parse::<u64>()
        .map_err(|_| invalid_input(format!("{name} must be an unsigned integer")))
}

fn optional_alert_severity(
    args: &[String],
    name: &str,
) -> Result<Option<OperationAlertSeverity>, io::Error> {
    option_value(args, name)
        .map(|value| match value.as_str() {
            "Critical" | "critical" => Ok(OperationAlertSeverity::Critical),
            "Warning" | "warning" => Ok(OperationAlertSeverity::Warning),
            _ => Err(invalid_input(format!(
                "{name} must be Critical or Warning"
            ))),
        })
        .transpose()
}

fn required_role(args: &[String], name: &str) -> Result<RbacRole, io::Error> {
    let value = required_option(args, name)?;
    match value.as_str() {
        "Viewer" | "viewer" => Ok(RbacRole::Viewer),
        "Operator" | "operator" => Ok(RbacRole::Operator),
        "BreakGlassOperator" | "break-glass-operator" | "break_glass_operator" => {
            Ok(RbacRole::BreakGlassOperator)
        }
        "Admin" | "admin" => Ok(RbacRole::Admin),
        "Auditor" | "auditor" => Ok(RbacRole::Auditor),
        _ => Err(invalid_input(format!(
            "{name} must be Viewer, Operator, BreakGlassOperator, Admin, or Auditor"
        ))),
    }
}

fn required_bool(args: &[String], name: &str) -> Result<bool, io::Error> {
    let value = required_option(args, name)?;
    match value.as_str() {
        "true" | "True" | "1" => Ok(true),
        "false" | "False" | "0" => Ok(false),
        _ => Err(invalid_input(format!("{name} must be true or false"))),
    }
}

fn required_worker_schedule_target(
    args: &[String],
    name: &str,
) -> Result<WorkerScheduleTarget, io::Error> {
    let value = required_option(args, name)?;
    match value.as_str() {
        "operation-summary-retention" | "operation_summary_retention" => {
            Ok(WorkerScheduleTarget::OperationSummaryRetention)
        }
        "operation-alert" | "operation_alert" => Ok(WorkerScheduleTarget::OperationAlert),
        "journal-gc" | "journal_gc" => Ok(WorkerScheduleTarget::JournalGc),
        "drift-audit" | "drift_audit" => Ok(WorkerScheduleTarget::DriftAudit),
        _ => Err(invalid_input(format!(
            "{name} must be operation-summary-retention, operation-alert, journal-gc, or drift-audit"
        ))),
    }
}

fn request_id(args: &[String], prefix: &str) -> String {
    option_value(args, "--request-id")
        .unwrap_or_else(|| format!("{prefix}-{}", uuid::Uuid::new_v4()))
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn print_usage() {
    eprintln!(
        "usage:\n  aria-underlay-ops list-in-doubt --journal-root <dir> [--device-id <id>]\n  aria-underlay-ops force-resolve --journal-root <dir> --operation-summary-path <file> --tx-id <tx> --operator <name> --reason <text> --break-glass [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops list-operations --operation-summary-path <file> [--attention-required] [--action <name>] [--result <result>] [--device-id <id>] [--tx-id <tx>] [--limit <n>]\n  aria-underlay-ops operation-summary --operation-summary-path <file> [--attention-required] [--action <name>] [--result <result>] [--device-id <id>] [--tx-id <tx>] [--limit <n>]\n  aria-underlay-ops list-alerts --operation-alert-path <file> [--alert-state-path <file>] [--severity Critical|Warning] [--limit <n>]\n  aria-underlay-ops alert-summary --operation-alert-path <file> [--alert-state-path <file>] [--severity Critical|Warning]\n  aria-underlay-ops ack-alert --alert-state-path <file> --product-audit-path <file> --dedupe-key <key> --operator <name> --role <role> --reason <text> [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops resolve-alert --alert-state-path <file> --product-audit-path <file> --dedupe-key <key> --operator <name> --role <role> --reason <text> [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops suppress-alert --alert-state-path <file> --product-audit-path <file> --dedupe-key <key> --operator <name> --role <role> --reason <text> [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops expire-alert --alert-state-path <file> --product-audit-path <file> --dedupe-key <key> --operator <name> --role <role> --reason <text> [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops set-summary-retention --worker-config-path <file> --product-audit-path <file> --operator <name> --role Admin --reason <text> [--max-records <n>] [--max-bytes <n>] [--max-rotated-files <n>] [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops set-gc-retention --worker-config-path <file> --product-audit-path <file> --operator <name> --role Admin --reason <text> --committed-days <n> --rolled-back-days <n> --failed-days <n> --rollback-artifact-days <n> --max-artifacts-per-device <n> [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops set-worker-schedule --worker-config-path <file> --product-audit-path <file> --operator <name> --role Admin --reason <text> --target <target> --interval-secs <n> --run-immediately true|false [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops check-worker-config --worker-config-path <file> [--strict-paths]\n  aria-underlay-ops worker-reload-status --checkpoint-path <file>"
    );
}
