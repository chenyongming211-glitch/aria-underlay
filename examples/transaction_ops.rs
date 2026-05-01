use std::error::Error;
use std::io;
use std::sync::Arc;

use aria_underlay::api::force_resolve::ForceResolveTransactionRequest;
use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::transactions::ListInDoubtTransactionsRequest;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::DeviceInventory;
use aria_underlay::model::DeviceId;
use aria_underlay::telemetry::JsonFileOperationSummaryStore;
use aria_underlay::tx::JsonFileTxJournalStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Err(invalid_input("missing command").into());
    };

    match command {
        "list-in-doubt" => list_in_doubt(&args[1..]).await,
        "force-resolve" => force_resolve(&args[1..]).await,
        "list-operations" => list_operations(&args[1..]).await,
        "operation-summary" => operation_summary(&args[1..]).await,
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
    let service = service_for_journal_root(journal_root);
    let response = service
        .list_in_doubt_transactions(ListInDoubtTransactionsRequest { device_id })
        .await?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn force_resolve(args: &[String]) -> Result<(), Box<dyn Error>> {
    let journal_root = required_option(args, "--journal-root")?;
    let tx_id = required_option(args, "--tx-id")?.to_string();
    let operator = required_option(args, "--operator")?.to_string();
    let reason = required_option(args, "--reason")?.to_string();
    let request_id = option_value(args, "--request-id")
        .unwrap_or_else(|| format!("force-resolve-{}", uuid::Uuid::new_v4()));
    let trace_id = option_value(args, "--trace-id");
    let service = service_for_journal_root(journal_root);
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

fn service_for_journal_root(journal_root: String) -> AriaUnderlayService {
    AriaUnderlayService::new_with_journal(
        DeviceInventory::default(),
        Arc::new(JsonFileTxJournalStore::new(journal_root)),
    )
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

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn print_usage() {
    eprintln!(
        "usage:\n  cargo run --example transaction_ops -- list-in-doubt --journal-root <dir> [--device-id <id>]\n  cargo run --example transaction_ops -- force-resolve --journal-root <dir> --tx-id <tx> --operator <name> --reason <text> --break-glass [--request-id <id>] [--trace-id <id>]\n  cargo run --example transaction_ops -- list-operations --operation-summary-path <file> [--attention-required] [--action <name>] [--result <result>] [--device-id <id>] [--tx-id <tx>] [--limit <n>]\n  cargo run --example transaction_ops -- operation-summary --operation-summary-path <file> [--attention-required] [--action <name>] [--result <result>] [--device-id <id>] [--tx-id <tx>] [--limit <n>]"
    );
}
