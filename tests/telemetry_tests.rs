use std::fs;
use std::sync::Arc;

use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::request::DriftAuditRequest;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::api::response::{ApplyStatus, DeviceApplyResult};
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};
use aria_underlay::proto::adapter;
use aria_underlay::state::drift::{DriftFinding, DriftReport, DriftType};
use aria_underlay::telemetry::{
    AuditRecord, EventSink, InMemoryEventSink, InMemoryOperationSummaryStore,
    JsonFileOperationSummaryStore, MetricName, Metrics, OperationSummary,
    OperationSummaryRetentionPolicy, OperationSummaryStore, RecordingEventSink, UnderlayEvent,
    UnderlayEventKind,
};
use aria_underlay::{UnderlayError, UnderlayResult};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::tx::{TransactionStrategy, TxPhase};
use aria_underlay::worker::gc::JournalGcReport;

mod common;

use common::{start_test_adapter, TestAdapter};

#[test]
fn transaction_result_event_maps_committed_phase_to_completed_kind() {
    let event = UnderlayEvent::transaction_result(
        "req-1",
        "trace-1",
        "tx-1",
        Some(DeviceId("leaf-a".into())),
        TxPhase::Committed,
        Some(TransactionStrategy::ConfirmedCommit),
        "success",
    );

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionCompleted);
    assert_eq!(event.tx_id.as_deref(), Some("tx-1"));
    assert_eq!(event.device_id.as_ref().map(|id| id.0.as_str()), Some("leaf-a"));
    assert_eq!(event.result.as_deref(), Some("success"));
}

#[test]
fn transaction_result_event_maps_in_doubt_phase_to_in_doubt_kind() {
    let event = UnderlayEvent::transaction_result(
        "req-1",
        "trace-1",
        "tx-1",
        None,
        TxPhase::InDoubt,
        Some(TransactionStrategy::CandidateCommit),
        "in_doubt",
    )
    .with_error("TX_IN_DOUBT", "candidate commit result is unknown");

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionInDoubt);
    assert_eq!(event.error_code.as_deref(), Some("TX_IN_DOUBT"));
}

#[test]
fn audit_record_preserves_traceable_transaction_fields() {
    let event = UnderlayEvent::transaction_result(
        "req-1",
        "trace-1",
        "tx-1",
        Some(DeviceId("leaf-a".into())),
        TxPhase::RolledBack,
        Some(TransactionStrategy::ConfirmedCommit),
        "rolled_back",
    );

    let record = AuditRecord::from_event(&event);

    assert_eq!(record.request_id, "req-1");
    assert_eq!(record.trace_id, "trace-1");
    assert_eq!(record.tx_id.as_deref(), Some("tx-1"));
    assert_eq!(record.action, "transaction.completed");
    assert_eq!(record.result, "rolled_back");
}

#[test]
fn audit_record_maps_force_resolved_transaction_event() {
    let event = UnderlayEvent::transaction_force_resolved(
        "req-force",
        "trace-force",
        "tx-force",
        TxPhase::InDoubt,
        &[DeviceId("leaf-a".into()), DeviceId("leaf-b".into())],
        "netops-a",
        "validated device state out of band",
    );

    let record = AuditRecord::from_event(&event);

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionForceResolved);
    assert_eq!(event.phase, Some(TxPhase::ForceResolved));
    assert_eq!(event.result.as_deref(), Some("force_resolved"));
    assert_eq!(event.fields.get("operator").map(String::as_str), Some("netops-a"));
    assert_eq!(
        event.fields.get("device_count").map(String::as_str),
        Some("2")
    );
    assert_eq!(record.action, "transaction.force_resolved");
    assert_eq!(record.result, "force_resolved");
    assert_eq!(record.tx_id.as_deref(), Some("tx-force"));
}

#[test]
fn device_apply_result_maps_to_traceable_transaction_event() {
    let result = DeviceApplyResult {
        device_id: DeviceId("leaf-a".into()),
        changed: true,
        status: ApplyStatus::InDoubt,
        tx_id: Some("tx-1".into()),
        strategy: Some(TransactionStrategy::CandidateCommit),
        error_code: Some("TX_IN_DOUBT".into()),
        error_message: Some("final state is unknown".into()),
        warnings: vec!["manual recovery required".into()],
    };

    let event = UnderlayEvent::from_device_apply_result("req-1", "trace-1", &result)
        .expect("changed apply result with tx_id should produce event");

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionInDoubt);
    assert_eq!(event.tx_id.as_deref(), Some("tx-1"));
    assert_eq!(event.device_id.as_ref().map(|id| id.0.as_str()), Some("leaf-a"));
    assert_eq!(event.result.as_deref(), Some("in_doubt"));
    assert_eq!(event.error_code.as_deref(), Some("TX_IN_DOUBT"));
    assert_eq!(event.fields.get("warning_count").map(String::as_str), Some("1"));
}

#[test]
fn noop_apply_result_does_not_create_transaction_event() {
    let result = DeviceApplyResult {
        device_id: DeviceId("leaf-a".into()),
        changed: false,
        status: ApplyStatus::NoOpSuccess,
        tx_id: None,
        strategy: None,
        error_code: None,
        error_message: None,
        warnings: Vec::new(),
    };

    assert!(UnderlayEvent::from_device_apply_result("req-1", "trace-1", &result).is_none());
}

#[test]
fn drift_report_maps_to_drift_event() {
    let report = DriftReport {
        device_id: DeviceId("leaf-a".into()),
        drift_detected: true,
        findings: vec![DriftFinding {
            drift_type: DriftType::MissingVlan,
            path: "vlans.100".into(),
            expected: Some("id=100".into()),
            actual: None,
        }],
        warnings: Vec::new(),
    };

    let event = UnderlayEvent::drift_detected("req-1", "trace-1", &report);

    assert_eq!(event.kind, UnderlayEventKind::UnderlayDriftDetected);
    assert_eq!(event.device_id.as_ref().map(|id| id.0.as_str()), Some("leaf-a"));
    assert_eq!(event.result.as_deref(), Some("drift_detected"));
    assert_eq!(event.fields.get("finding_count").map(String::as_str), Some("1"));
    assert_eq!(event.fields.get("first_path").map(String::as_str), Some("vlans.100"));
}

#[tokio::test]
async fn service_emits_drift_event_to_configured_sink() {
    let adapter_endpoint = start_drift_adapter().await;
    let inventory = telemetry_inventory(adapter_endpoint);
    let sink = Arc::new(InMemoryEventSink::default());
    let service = AriaUnderlayService::new(inventory).with_event_sink(sink.clone());

    let response = service
        .run_drift_audit(DriftAuditRequest {
            device_ids: vec![DeviceId("leaf-a".into())],
        })
        .await
        .expect("drift audit should complete");

    assert_eq!(response.drifted_devices, vec![DeviceId("leaf-a".into())]);
    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayDriftDetected);
    assert_eq!(
        events[0].device_id.as_ref().map(|id| id.0.as_str()),
        Some("leaf-a")
    );
    assert_eq!(events[0].result.as_deref(), Some("drift_detected"));
    assert_eq!(events[1].kind, UnderlayEventKind::UnderlayDriftAuditCompleted);
    assert_eq!(events[1].result.as_deref(), Some("drift_detected"));
    assert_eq!(
        events[1].fields.get("drifted_device_count").map(String::as_str),
        Some("1")
    );
}

#[tokio::test]
async fn service_records_queryable_operation_summaries_while_emitting_events() {
    let adapter_endpoint = start_drift_adapter().await;
    let inventory = telemetry_inventory(adapter_endpoint);
    let sink = Arc::new(InMemoryEventSink::default());
    let service = AriaUnderlayService::new(inventory).with_event_sink(sink.clone());

    service
        .run_drift_audit(DriftAuditRequest {
            device_ids: vec![DeviceId("leaf-a".into())],
        })
        .await
        .expect("drift audit should complete");

    let events = sink.events();
    assert_eq!(events.len(), 2);

    let all = service
        .list_operation_summaries(ListOperationSummariesRequest::default())
        .await
        .expect("operation summaries should be queryable");
    assert_eq!(all.summaries.len(), 2);
    assert_eq!(all.summaries[0].action, "drift.detected");
    assert_eq!(all.summaries[1].action, "drift.audit_completed");

    let attention = service
        .list_operation_summaries(ListOperationSummariesRequest {
            attention_required_only: true,
            ..Default::default()
        })
        .await
        .expect("attention summaries should be queryable");
    assert_eq!(attention.summaries.len(), 2);

    let completed = service
        .list_operation_summaries(ListOperationSummariesRequest {
            action: Some("drift.audit_completed".into()),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .expect("operation summaries should be filterable");
    assert_eq!(completed.summaries.len(), 1);
    assert_eq!(completed.summaries[0].result, "drift_detected");
    assert_eq!(
        completed.summaries[0]
            .fields
            .get("drifted_device_count")
            .map(String::as_str),
        Some("1")
    );
}

#[tokio::test]
async fn service_emits_recovery_completion_event_to_configured_sink() {
    let sink = Arc::new(InMemoryEventSink::default());
    let service = AriaUnderlayService::new(DeviceInventory::default()).with_event_sink(sink.clone());

    let report = service
        .recover_pending_transactions()
        .await
        .expect("empty recovery scan should complete");

    assert_eq!(report.pending, 0);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayRecoveryCompleted);
    assert_eq!(events[0].result.as_deref(), Some("completed"));
}

#[test]
fn metrics_records_transaction_outcomes() {
    let mut metrics = Metrics::default();

    metrics.record_transaction_status(&ApplyStatus::Success);
    metrics.record_transaction_status(&ApplyStatus::Failed);
    metrics.record_transaction_status(&ApplyStatus::RolledBack);
    metrics.record_transaction_status(&ApplyStatus::InDoubt);

    let samples = metrics.samples();

    assert_eq!(metric_value(&samples, MetricName::TransactionTotal), 4.0);
    assert_eq!(metric_value(&samples, MetricName::TransactionFailedTotal), 1.0);
    assert_eq!(metric_value(&samples, MetricName::TransactionRollbackTotal), 1.0);
    assert_eq!(metric_value(&samples, MetricName::TransactionInDoubtTotal), 1.0);
}

#[test]
fn metrics_record_operator_events() {
    let mut metrics = Metrics::default();
    let recovery = UnderlayEvent::recovery_completed(
        "req-recovery",
        "trace-recovery",
        &RecoveryReport {
            recovered: 0,
            in_doubt: 1,
            pending: 1,
            tx_ids: vec!["tx-1".into()],
            decisions: Vec::new(),
        },
    );
    let drift = UnderlayEvent::drift_audit_completed(
        "req-drift",
        "trace-drift",
        2,
        &[DeviceId("leaf-a".into())],
    );
    let gc = UnderlayEvent::journal_gc_completed(
        "req-gc",
        "trace-gc",
        &JournalGcReport {
            journals_deleted: 1,
            journals_retained: 2,
            artifacts_deleted: 1,
            journal_deleted_tx_ids: vec!["tx-old".into()],
            artifact_deleted_refs: vec!["leaf-a/tx-old".into()],
        },
    );
    let force = UnderlayEvent::transaction_force_resolved(
        "req-force",
        "trace-force",
        "tx-force",
        TxPhase::InDoubt,
        &[DeviceId("leaf-a".into())],
        "netops-a",
        "validated out of band",
    );

    for event in [&recovery, &drift, &gc, &force] {
        metrics.record_event(event);
    }

    let samples = metrics.samples();
    assert_eq!(metric_value(&samples, MetricName::OperationRecoveryTotal), 1.0);
    assert_eq!(
        metric_value(&samples, MetricName::OperationRecoveryInDoubtTotal),
        1.0
    );
    assert_eq!(metric_value(&samples, MetricName::OperationDriftAuditTotal), 1.0);
    assert_eq!(
        metric_value(&samples, MetricName::OperationDriftDetectedTotal),
        1.0
    );
    assert_eq!(metric_value(&samples, MetricName::OperationJournalGcTotal), 1.0);
    assert_eq!(
        metric_value(&samples, MetricName::OperationJournalGcDeletedTotal),
        1.0
    );
    assert_eq!(
        metric_value(&samples, MetricName::OperationForceResolveTotal),
        1.0
    );
}

#[test]
fn metrics_record_audit_write_failures() {
    let mut metrics = Metrics::default();
    let event = UnderlayEvent::audit_write_failed(
        "req-audit",
        "trace-audit",
        "recovery.completed",
        "operation summary io error: disk full",
    );

    metrics.record_event(&event);

    assert_eq!(
        metric_value(&metrics.samples(), MetricName::OperationAuditWriteFailedTotal),
        1.0
    );
}

#[test]
fn operation_summary_store_keeps_queryable_operator_view() {
    let store = InMemoryOperationSummaryStore::default();
    let recovery = UnderlayEvent::recovery_completed(
        "req-recovery",
        "trace-recovery",
        &RecoveryReport {
            recovered: 0,
            in_doubt: 1,
            pending: 1,
            tx_ids: vec!["tx-1".into()],
            decisions: Vec::new(),
        },
    );
    let gc = UnderlayEvent::journal_gc_completed(
        "req-gc",
        "trace-gc",
        &JournalGcReport {
            journals_deleted: 0,
            journals_retained: 3,
            artifacts_deleted: 0,
            ..Default::default()
        },
    );
    let force = UnderlayEvent::transaction_force_resolved(
        "req-force",
        "trace-force",
        "tx-force",
        TxPhase::InDoubt,
        &[DeviceId("leaf-a".into())],
        "netops-a",
        "validated out of band",
    );

    store.record_event(&recovery).expect("recovery summary should record");
    store.record_event(&gc).expect("gc summary should record");
    store.record_event(&force).expect("force summary should record");

    let summaries = store.list().expect("summaries should be queryable");
    assert_eq!(summaries.len(), 3);
    assert_eq!(summaries[0].action, "recovery.completed");
    assert_eq!(summaries[1].action, "journal.gc_completed");
    assert_eq!(summaries[2].action, "transaction.force_resolved");

    let attention = store
        .list_attention_required()
        .expect("attention summaries should be queryable");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].action, "recovery.completed");
    assert_eq!(attention[0].result, "in_doubt");
}

#[test]
fn recording_event_sink_emits_audit_write_failed_when_summary_persistence_fails() {
    let inner = Arc::new(InMemoryEventSink::default());
    let sink = RecordingEventSink::new(inner.clone(), Arc::new(FailingOperationSummaryStore));
    let event = recovery_event(1);

    sink.emit(event.clone());

    let events = inner.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayAuditWriteFailed);
    assert_eq!(events[0].request_id, "req-1");
    assert_eq!(events[0].trace_id, "trace-1");
    assert_eq!(events[0].result.as_deref(), Some("failed"));
    assert_eq!(
        events[0].error_code.as_deref(),
        Some("OPERATION_SUMMARY_WRITE_FAILED")
    );
    assert_eq!(
        events[0].fields.get("failed_action").map(String::as_str),
        Some("recovery.completed")
    );
    assert!(
        events[0]
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("forced operation summary failure")
    );
    assert_eq!(events[1], event);
}

#[test]
fn json_file_operation_summary_store_persists_operator_view_across_restarts() {
    let path = temp_operation_summary_path("persist-restart");
    let store = JsonFileOperationSummaryStore::new(&path);
    let recovery = UnderlayEvent::recovery_completed(
        "req-recovery",
        "trace-recovery",
        &RecoveryReport {
            recovered: 0,
            in_doubt: 1,
            pending: 0,
            tx_ids: vec!["tx-1".into()],
            decisions: Vec::new(),
        },
    );
    let gc = UnderlayEvent::journal_gc_completed(
        "req-gc",
        "trace-gc",
        &JournalGcReport {
            journals_deleted: 1,
            journals_retained: 2,
            artifacts_deleted: 0,
            journal_deleted_tx_ids: vec!["tx-old".into()],
            artifact_deleted_refs: Vec::new(),
        },
    );
    let non_operator = UnderlayEvent::transaction_result(
        "req-ok",
        "trace-ok",
        "tx-ok",
        Some(DeviceId("leaf-a".into())),
        TxPhase::Committed,
        Some(TransactionStrategy::ConfirmedCommit),
        "success",
    );

    store
        .record_event(&recovery)
        .expect("recovery summary should persist");
    store.record_event(&gc).expect("gc summary should persist");
    store
        .record_event(&non_operator)
        .expect("non-operator event should be ignored without error");

    let restarted = JsonFileOperationSummaryStore::new(&path);
    let summaries = restarted
        .list()
        .expect("restarted operation summary store should list records");
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].action, "recovery.completed");
    assert_eq!(summaries[0].result, "in_doubt");
    assert_eq!(summaries[1].action, "journal.gc_completed");
    assert_eq!(
        summaries[1]
            .fields
            .get("journal_deleted_tx_ids")
            .map(String::as_str),
        Some("tx-old")
    );

    let attention = restarted
        .list_attention_required()
        .expect("attention summaries should be queryable after restart");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].action, "recovery.completed");

    remove_operation_summary_path(&path);
}

#[test]
fn json_file_operation_summary_store_fails_closed_on_corrupt_record() {
    let path = temp_operation_summary_path("corrupt-record");
    fs::create_dir_all(path.parent().expect("summary path should have parent"))
        .expect("summary parent should be created");
    fs::write(&path, "{not-json}\n").expect("corrupt summary record should be written");

    let err = JsonFileOperationSummaryStore::new(&path)
        .list()
        .expect_err("corrupt operation summary record should fail closed");
    let message = format!("{err}");

    assert!(
        message.contains("operation summary") && message.contains("line 1"),
        "unexpected corrupt summary error: {message}"
    );

    remove_operation_summary_path(&path);
}

#[test]
fn json_file_operation_summary_store_compacts_to_newest_records_and_rotates_archive() {
    let path = temp_operation_summary_path("compact-records");
    let store = JsonFileOperationSummaryStore::new(&path);
    for index in 1..=3 {
        store
            .record_event(&recovery_event(index))
            .expect("operation summary should persist before compaction");
    }

    let report = store
        .compact(OperationSummaryRetentionPolicy {
            max_records: Some(2),
            max_bytes: None,
            max_rotated_files: 2,
        })
        .expect("operation summaries should compact");

    assert!(report.compacted);
    assert_eq!(report.records_before, 3);
    assert_eq!(report.records_after, 2);
    assert_eq!(report.records_dropped, 1);
    assert_eq!(report.rotated_files, 1);

    let summaries = store.list().expect("compacted summaries should list");
    assert_eq!(summary_request_ids(&summaries), vec!["req-2", "req-3"]);

    let archive_payload =
        fs::read_to_string(operation_summary_archive_path(&path, 1))
            .expect("pre-compaction JSONL should be archived");
    assert!(archive_payload.contains("req-1"));
    assert!(archive_payload.contains("req-2"));
    assert!(archive_payload.contains("req-3"));

    remove_operation_summary_path(&path);
}

#[test]
fn json_file_operation_summary_store_compacts_to_max_bytes_without_partial_lines() {
    let path = temp_operation_summary_path("compact-bytes");
    let store = JsonFileOperationSummaryStore::new(&path);
    let events = (1..=4).map(padded_recovery_event).collect::<Vec<_>>();
    for event in &events {
        store
            .record_event(event)
            .expect("operation summary should persist before byte compaction");
    }
    let max_bytes = events[2..]
        .iter()
        .map(operation_summary_jsonl_len)
        .sum::<usize>() as u64;

    let report = store
        .compact(OperationSummaryRetentionPolicy {
            max_records: None,
            max_bytes: Some(max_bytes),
            max_rotated_files: 0,
        })
        .expect("operation summaries should compact by byte cap");

    assert!(report.compacted);
    assert!(report.bytes_after <= max_bytes);
    assert_eq!(report.records_after, 2);
    let payload = fs::read_to_string(&path).expect("compacted JSONL should be readable");
    assert!(payload.lines().all(|line| !line.trim().is_empty()));

    let summaries = store.list().expect("byte-compacted summaries should list");
    assert_eq!(summary_request_ids(&summaries), vec!["req-3", "req-4"]);

    remove_operation_summary_path(&path);
}

#[test]
fn json_file_operation_summary_compaction_fails_closed_on_corrupt_record() {
    let path = temp_operation_summary_path("compact-corrupt-record");
    fs::create_dir_all(path.parent().expect("summary path should have parent"))
        .expect("summary parent should be created");
    fs::write(&path, "{not-json}\n").expect("corrupt summary record should be written");

    let err = JsonFileOperationSummaryStore::new(&path)
        .compact(OperationSummaryRetentionPolicy {
            max_records: Some(1),
            max_bytes: None,
            max_rotated_files: 1,
        })
        .expect_err("corrupt operation summary compaction should fail closed");
    let message = format!("{err}");

    assert!(
        message.contains("operation summary") && message.contains("line 1"),
        "unexpected corrupt compaction error: {message}"
    );
    assert_eq!(
        fs::read_to_string(&path).expect("corrupt active file should remain"),
        "{not-json}\n"
    );
    assert!(
        !operation_summary_archive_path(&path, 1).exists(),
        "corrupt compaction should not rotate unreadable input"
    );

    remove_operation_summary_path(&path);
}

#[tokio::test]
async fn service_can_record_operation_summaries_to_persistent_store() {
    let adapter_endpoint = start_drift_adapter().await;
    let inventory = telemetry_inventory(adapter_endpoint);
    let path = temp_operation_summary_path("service-persistent-store");
    let persistent_store = Arc::new(JsonFileOperationSummaryStore::new(&path));
    let service = AriaUnderlayService::new(inventory)
        .with_operation_summary_store(persistent_store.clone());

    service
        .run_drift_audit(DriftAuditRequest {
            device_ids: vec![DeviceId("leaf-a".into())],
        })
        .await
        .expect("drift audit should complete");

    let restarted = JsonFileOperationSummaryStore::new(&path);
    let summaries = restarted
        .list()
        .expect("persistent summaries should survive store recreation");
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].action, "drift.detected");
    assert_eq!(summaries[1].action, "drift.audit_completed");

    remove_operation_summary_path(&path);
}

fn metric_value(samples: &[aria_underlay::telemetry::MetricSample], name: MetricName) -> f64 {
    samples
        .iter()
        .find(|sample| sample.name == name)
        .map(|sample| sample.value)
        .unwrap_or_default()
}

fn telemetry_inventory(adapter_endpoint: String) -> DeviceInventory {
    let inventory = DeviceInventory::default();
    inventory
        .insert(DeviceInfo {
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            id: DeviceId("leaf-a".into()),
            management_ip: "127.0.0.1".into(),
            management_port: 830,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref: "local/leaf-a".into(),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint,
            lifecycle_state: DeviceLifecycleState::Ready,
        })
        .expect("telemetry test device should be inserted");
    inventory
}

async fn start_drift_adapter() -> String {
    start_test_adapter(TestAdapter {
        current_state: Some(adapter::ObservedDeviceState {
            device_id: "leaf-a".into(),
            vlans: Vec::new(),
            interfaces: Vec::new(),
        }),
        current_warnings: vec!["manual change detected by adapter".into()],
        ..Default::default()
    })
    .await
}

fn temp_operation_summary_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!(
            "aria-underlay-operation-summary-{name}-{}",
            uuid::Uuid::new_v4()
        ))
        .join("summaries.jsonl")
}

fn remove_operation_summary_path(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        fs::remove_dir_all(parent).ok();
    }
}

fn recovery_event(index: usize) -> UnderlayEvent {
    UnderlayEvent::recovery_completed(
        format!("req-{index}"),
        format!("trace-{index}"),
        &RecoveryReport {
            recovered: index,
            in_doubt: 0,
            pending: 0,
            tx_ids: vec![format!("tx-{index}")],
            decisions: Vec::new(),
        },
    )
}

fn padded_recovery_event(index: usize) -> UnderlayEvent {
    let mut event = recovery_event(index);
    event
        .fields
        .insert("padding".into(), format!("padding-{index}-{}", "x".repeat(64)));
    event
}

fn summary_request_ids(summaries: &[OperationSummary]) -> Vec<&str> {
    summaries
        .iter()
        .map(|summary| summary.request_id.as_str())
        .collect()
}

fn operation_summary_jsonl_len(event: &UnderlayEvent) -> usize {
    let summary = OperationSummary::from_event(event).expect("event should map to summary");
    serde_json::to_vec(&summary)
        .expect("summary should serialize")
        .len()
        + 1
}

fn operation_summary_archive_path(path: &std::path::Path, generation: usize) -> std::path::PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("summary path should have file name");
    path.with_file_name(format!("{file_name}.{generation}"))
}

#[derive(Debug)]
struct FailingOperationSummaryStore;

impl OperationSummaryStore for FailingOperationSummaryStore {
    fn record_event(&self, _event: &UnderlayEvent) -> UnderlayResult<()> {
        Err(UnderlayError::Internal(
            "forced operation summary failure".into(),
        ))
    }

    fn list(&self) -> UnderlayResult<Vec<OperationSummary>> {
        Ok(Vec::new())
    }
}
