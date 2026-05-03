use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::Arc;

use serde::Serialize;

use crate::api::force_resolve::ForceResolveTransactionRequest;
use crate::api::operations::ListOperationSummariesRequest;
use crate::api::transactions::ListInDoubtTransactionsRequest;
use crate::api::{AriaUnderlayService, UnderlayService};
use crate::device::DeviceInventory;
use crate::model::DeviceId;
use crate::telemetry::{
    JsonFileOperationAlertSink, JsonFileOperationSummaryStore, OperationAlert,
    OperationAlertSeverity,
};
use crate::tx::JsonFileTxJournalStore;

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
    let limit = optional_usize(args, "--limit")?;
    let returned_alerts = limit_alerts(&alerts, limit);
    let response = ListOperationAlertsResponse {
        overview: OperationAlertOverview::from_alerts(&alerts, returned_alerts.len()),
        alerts: returned_alerts,
    };

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn alert_summary(args: &[String]) -> Result<(), Box<dyn Error>> {
    let operation_alert_path = required_option(args, "--operation-alert-path")?;
    let alerts = filtered_alerts(args, operation_alert_path)?;
    let overview = OperationAlertOverview::from_alerts(&alerts, alerts.len());

    println!("{}", serde_json::to_string_pretty(&overview)?);
    Ok(())
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

fn limit_alerts(alerts: &[OperationAlert], limit: Option<usize>) -> Vec<OperationAlert> {
    alerts
        .iter()
        .take(limit.unwrap_or(alerts.len()))
        .cloned()
        .collect()
}

#[derive(Debug, Serialize)]
struct ListOperationAlertsResponse {
    alerts: Vec<OperationAlert>,
    overview: OperationAlertOverview,
}

#[derive(Debug, Default, Serialize)]
struct OperationAlertOverview {
    matched_alerts: usize,
    returned_alerts: usize,
    critical: usize,
    warning: usize,
    by_action: BTreeMap<String, usize>,
    by_result: BTreeMap<String, usize>,
    by_device: BTreeMap<String, usize>,
}

impl OperationAlertOverview {
    fn from_alerts(alerts: &[OperationAlert], returned_alerts: usize) -> Self {
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
        }

        overview
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

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn print_usage() {
    eprintln!(
        "usage:\n  aria-underlay-ops list-in-doubt --journal-root <dir> [--device-id <id>]\n  aria-underlay-ops force-resolve --journal-root <dir> --operation-summary-path <file> --tx-id <tx> --operator <name> --reason <text> --break-glass [--request-id <id>] [--trace-id <id>]\n  aria-underlay-ops list-operations --operation-summary-path <file> [--attention-required] [--action <name>] [--result <result>] [--device-id <id>] [--tx-id <tx>] [--limit <n>]\n  aria-underlay-ops operation-summary --operation-summary-path <file> [--attention-required] [--action <name>] [--result <result>] [--device-id <id>] [--tx-id <tx>] [--limit <n>]\n  aria-underlay-ops list-alerts --operation-alert-path <file> [--severity Critical|Warning] [--limit <n>]\n  aria-underlay-ops alert-summary --operation-alert-path <file> [--severity Critical|Warning]"
    );
}
