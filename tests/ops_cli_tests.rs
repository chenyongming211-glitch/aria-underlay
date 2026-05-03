use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use aria_underlay::model::DeviceId;
use aria_underlay::telemetry::{
    JsonFileOperationAlertSink, JsonFileOperationSummaryStore, JsonFileProductAuditStore,
    OperationAlert, OperationAlertSeverity, OperationAlertSink, UnderlayEvent,
};
use aria_underlay::tx::context::TxContext;
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::tx::recovery::RecoveryReport;

#[test]
fn ops_cli_prints_attention_required_operation_overview() {
    let temp = temp_test_dir("operation-overview");
    let summary_path = temp.join("summaries.jsonl");
    let store = JsonFileOperationSummaryStore::new(&summary_path);
    store
        .record_event(&UnderlayEvent::recovery_completed(
            "req-recovery",
            "trace-recovery",
            &RecoveryReport {
                recovered: 0,
                in_doubt: 1,
                pending: 0,
                tx_ids: vec!["tx-recovery".into()],
                decisions: Vec::new(),
            },
        ))
        .expect("attention-required operation summary should be written");
    store
        .record_event(&UnderlayEvent::recovery_completed(
            "req-clean",
            "trace-clean",
            &RecoveryReport {
                recovered: 1,
                in_doubt: 0,
                pending: 0,
                tx_ids: vec!["tx-clean".into()],
                decisions: Vec::new(),
            },
        ))
        .expect("clean operation summary should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_aria-underlay-ops"))
        .args([
            "operation-summary",
            "--operation-summary-path",
            summary_path.to_str().expect("summary path should be utf-8"),
            "--attention-required",
        ])
        .output()
        .expect("aria-underlay-ops should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("operation summary should be JSON");
    assert_eq!(payload["matched_records"], 1);
    assert_eq!(payload["returned_records"], 1);
    assert_eq!(payload["attention_required"], 1);
    assert_eq!(payload["by_result"]["in_doubt"], 1);

    fs::remove_dir_all(temp).ok();
}

#[test]
fn ops_cli_lists_and_summarizes_alerts() {
    let temp = temp_test_dir("alert-list");
    let alert_path = temp.join("alerts.jsonl");
    let sink = JsonFileOperationAlertSink::new(&alert_path);
    sink.deliver(&[
        alert("critical-key", OperationAlertSeverity::Critical, "transaction.in_doubt"),
        alert("warning-key", OperationAlertSeverity::Warning, "drift.detected"),
    ])
    .expect("alerts should be written");

    let list_output = Command::new(env!("CARGO_BIN_EXE_aria-underlay-ops"))
        .args([
            "list-alerts",
            "--operation-alert-path",
            alert_path.to_str().expect("alert path should be utf-8"),
            "--severity",
            "Critical",
            "--limit",
            "1",
        ])
        .output()
        .expect("aria-underlay-ops should run");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_payload: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).expect("alert list should be JSON");
    assert_eq!(list_payload["overview"]["matched_alerts"], 1);
    assert_eq!(list_payload["overview"]["returned_alerts"], 1);
    assert_eq!(list_payload["overview"]["critical"], 1);
    assert_eq!(list_payload["alerts"][0]["dedupe_key"], "critical-key");

    let summary_output = Command::new(env!("CARGO_BIN_EXE_aria-underlay-ops"))
        .args([
            "alert-summary",
            "--operation-alert-path",
            alert_path.to_str().expect("alert path should be utf-8"),
        ])
        .output()
        .expect("aria-underlay-ops should run");
    assert!(
        summary_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&summary_output.stderr)
    );
    let summary_payload: serde_json::Value =
        serde_json::from_slice(&summary_output.stdout).expect("alert summary should be JSON");
    assert_eq!(summary_payload["matched_alerts"], 2);
    assert_eq!(summary_payload["returned_alerts"], 2);
    assert_eq!(summary_payload["critical"], 1);
    assert_eq!(summary_payload["warning"], 1);

    fs::remove_dir_all(temp).ok();
}

#[test]
fn ops_cli_acknowledges_alert_and_enriches_alert_list() {
    let temp = temp_test_dir("alert-lifecycle");
    let alert_path = temp.join("alerts.jsonl");
    let alert_state_path = temp.join("alert-state.json");
    let product_audit_path = temp.join("product-audit.jsonl");
    JsonFileOperationAlertSink::new(&alert_path)
        .deliver(&[alert(
            "critical-key",
            OperationAlertSeverity::Critical,
            "transaction.in_doubt",
        )])
        .expect("alert should be written");

    let ack_output = Command::new(env!("CARGO_BIN_EXE_aria-underlay-ops"))
        .args([
            "ack-alert",
            "--alert-state-path",
            alert_state_path
                .to_str()
                .expect("alert state path should be utf-8"),
            "--product-audit-path",
            product_audit_path
                .to_str()
                .expect("product audit path should be utf-8"),
            "--dedupe-key",
            "critical-key",
            "--operator",
            "netops-a",
            "--role",
            "Operator",
            "--reason",
            "investigating current operation alert",
        ])
        .output()
        .expect("aria-underlay-ops should run");
    assert!(
        ack_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ack_output.stderr)
    );
    let ack_payload: serde_json::Value =
        serde_json::from_slice(&ack_output.stdout).expect("ack response should be JSON");
    assert_eq!(ack_payload["record"]["dedupe_key"], "critical-key");
    assert_eq!(ack_payload["record"]["status"], "Acknowledged");

    let list_output = Command::new(env!("CARGO_BIN_EXE_aria-underlay-ops"))
        .args([
            "list-alerts",
            "--operation-alert-path",
            alert_path.to_str().expect("alert path should be utf-8"),
            "--alert-state-path",
            alert_state_path
                .to_str()
                .expect("alert state path should be utf-8"),
        ])
        .output()
        .expect("aria-underlay-ops should run");
    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_payload: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).expect("alert list should be JSON");
    assert_eq!(list_payload["alerts"][0]["dedupe_key"], "critical-key");
    assert_eq!(
        list_payload["alerts"][0]["lifecycle"]["status"],
        "Acknowledged"
    );
    assert_eq!(list_payload["overview"]["acknowledged"], 1);

    let audit_records = JsonFileProductAuditStore::new(&product_audit_path)
        .list()
        .expect("product audit should be readable");
    assert_eq!(audit_records.len(), 1);
    assert_eq!(audit_records[0].action, "alert.acknowledged");
    assert_eq!(audit_records[0].operator_id.as_deref(), Some("netops-a"));

    fs::remove_dir_all(temp).ok();
}

#[test]
fn ops_cli_force_resolve_writes_journal_and_operation_summary() {
    let temp = temp_test_dir("force-resolve");
    let journal_root = temp.join("journal");
    let summary_path = temp.join("summaries.jsonl");
    let journal = JsonFileTxJournalStore::new(&journal_root);
    journal
        .put(&journal_record("tx-manual", TxPhase::InDoubt, "leaf-a"))
        .expect("in-doubt journal record should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_aria-underlay-ops"))
        .args([
            "force-resolve",
            "--journal-root",
            journal_root.to_str().expect("journal root should be utf-8"),
            "--operation-summary-path",
            summary_path.to_str().expect("summary path should be utf-8"),
            "--tx-id",
            "tx-manual",
            "--operator",
            "netops-a",
            "--reason",
            "verified running config out of band",
            "--break-glass",
        ])
        .output()
        .expect("aria-underlay-ops should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record = journal
        .get("tx-manual")
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert_eq!(record.phase, TxPhase::ForceResolved);

    let summaries = JsonFileOperationSummaryStore::new(&summary_path)
        .list()
        .expect("operation summary should be readable");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].action, "transaction.force_resolved");
    assert_eq!(summaries[0].result, "force_resolved");
    assert_eq!(summaries[0].tx_id.as_deref(), Some("tx-manual"));
    assert_eq!(
        summaries[0].fields.get("operator").map(String::as_str),
        Some("netops-a")
    );

    fs::remove_dir_all(temp).ok();
}

fn alert(
    dedupe_key: &str,
    severity: OperationAlertSeverity,
    action: &str,
) -> OperationAlert {
    OperationAlert {
        dedupe_key: dedupe_key.into(),
        severity,
        request_id: format!("req-{dedupe_key}"),
        trace_id: format!("trace-{dedupe_key}"),
        action: action.into(),
        result: if action == "transaction.in_doubt" {
            "in_doubt".into()
        } else {
            "drift_detected".into()
        },
        tx_id: Some(format!("tx-{dedupe_key}")),
        device_id: Some(DeviceId("leaf-a".into())),
        fields: BTreeMap::new(),
    }
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-ops-cli-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn journal_record(tx_id: &str, phase: TxPhase, device_id: &str) -> TxJournalRecord {
    TxJournalRecord::started(
        &TxContext {
            tx_id: tx_id.into(),
            request_id: format!("req-{tx_id}"),
            trace_id: format!("trace-{tx_id}"),
        },
        vec![DeviceId(device_id.into())],
    )
    .with_phase(phase)
}
