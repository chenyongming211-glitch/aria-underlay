use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use aria_underlay::model::{DeviceId, InterfaceConfig, VlanConfig};
use aria_underlay::state::{DeviceShadowState, ShadowStateStore};
use aria_underlay::telemetry::{
    InMemoryEventSink, InMemoryOperationAlertCheckpointStore, InMemoryOperationAlertSink,
    InMemoryOperationSummaryStore, OperationAlertSeverity, UnderlayEvent, UnderlayEventKind,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::worker::drift_auditor::{
    DriftAuditSchedule, DriftAuditSnapshot, DriftAuditWorker, DriftAuditor,
    DriftObservationSource,
};
use aria_underlay::worker::gc::{
    JournalGc, JournalGcReport, JournalGcSchedule, JournalGcWorker, RetentionPolicy,
};
use aria_underlay::worker::operation_alerts::{
    OperationAlertDeliverySchedule, OperationAlertDeliveryWorker,
};
use aria_underlay::worker::runtime::UnderlayWorkerRuntime;
use aria_underlay::{UnderlayError, UnderlayResult};
use async_trait::async_trait;

#[tokio::test]
async fn worker_runtime_runs_gc_and_drift_workers_under_one_shutdown() {
    let temp = temp_test_dir("runtime-runs-workers");
    let journal_root = temp.join("journal");
    let journal = JsonFileTxJournalStore::new(&journal_root);
    journal
        .put(&journal_record("tx-old", TxPhase::Committed, 100))
        .expect("old terminal journal should be stored");

    let sink = Arc::new(InMemoryEventSink::default());
    let gc_worker = JournalGcWorker::new(
        JournalGc::new(&journal_root).with_now_unix_secs(100 + 31 * 24 * 60 * 60),
        RetentionPolicy {
            committed_journal_retention_days: 30,
            rolled_back_journal_retention_days: 30,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 50,
        },
        sink.clone(),
    );
    let drift_worker = DriftAuditWorker::new(
        DriftAuditor::new(vec![DriftAuditSnapshot {
            expected: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
            observed: shadow_state("leaf-a", vec![], vec![]),
        }]),
        sink.clone(),
    );

    let report = UnderlayWorkerRuntime::new()
        .with_journal_gc(
            gc_worker,
            JournalGcSchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        )
        .with_drift_audit(
            drift_worker,
            DriftAuditSchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        )
        .run_until_shutdown(async {})
        .await
        .expect("runtime should run enabled workers and stop on shutdown");

    let gc_report = report
        .journal_gc
        .expect("runtime should include journal GC scheduler report");
    assert_eq!(gc_report.runs, 1);
    assert_eq!(
        gc_report
            .last_report
            .expect("journal GC should retain last report")
            .journal_deleted_tx_ids,
        vec!["tx-old".to_string()]
    );

    let drift_report = report
        .drift_audit
        .expect("runtime should include drift audit scheduler report");
    assert_eq!(drift_report.runs, 1);
    assert_eq!(
        drift_report
            .last_summary
            .expect("drift audit should retain last summary")
            .drifted_devices,
        vec![DeviceId("leaf-a".into())]
    );

    let events = sink.events();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == UnderlayEventKind::UnderlayJournalGcCompleted)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == UnderlayEventKind::UnderlayDriftDetected)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == UnderlayEventKind::UnderlayDriftAuditCompleted)
            .count(),
        1
    );

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn operation_alert_worker_delivers_only_new_attention_required_summaries() {
    let summary_store = Arc::new(InMemoryOperationSummaryStore::default());
    summary_store
        .record_event(&recovery_event("req-alert-1", 1))
        .expect("attention-required recovery summary should record");
    summary_store
        .record_event(&UnderlayEvent::transaction_result(
            "req-alert-2",
            "trace-alert-2",
            "tx-alert-2",
            Some(DeviceId("leaf-b".into())),
            TxPhase::InDoubt,
            None,
            "in_doubt",
        ))
        .expect("attention-required transaction summary should record");
    summary_store
        .record_event(&UnderlayEvent::journal_gc_completed(
            "req-gc",
            "trace-gc",
            &JournalGcReport::default(),
        ))
        .expect("non-attention operation summary should record");

    let sink = Arc::new(InMemoryOperationAlertSink::default());
    let checkpoint = Arc::new(InMemoryOperationAlertCheckpointStore::default());
    let worker =
        OperationAlertDeliveryWorker::new(summary_store, sink.clone(), checkpoint.clone());

    let first = worker
        .run_once()
        .expect("first alert delivery should succeed");
    let second = worker
        .run_once()
        .expect("second alert delivery should be idempotent");

    assert_eq!(first.scanned_attention_required, 2);
    assert_eq!(first.newly_delivered, 2);
    assert_eq!(second.scanned_attention_required, 2);
    assert_eq!(second.newly_delivered, 0);

    let alerts = sink.alerts();
    assert_eq!(alerts.len(), 2);
    assert!(
        alerts
            .iter()
            .all(|alert| alert.severity == OperationAlertSeverity::Critical)
    );
    assert!(
        checkpoint
            .delivered_keys()
            .expect("checkpoint should be readable")
            .contains(&alerts[0].dedupe_key)
    );
}

#[tokio::test]
async fn worker_runtime_runs_operation_alert_worker_under_one_shutdown() {
    let summary_store = Arc::new(InMemoryOperationSummaryStore::default());
    summary_store
        .record_event(&recovery_event("req-alert-runtime", 1))
        .expect("attention-required summary should record");
    let sink = Arc::new(InMemoryOperationAlertSink::default());
    let checkpoint = Arc::new(InMemoryOperationAlertCheckpointStore::default());

    let report = UnderlayWorkerRuntime::new()
        .with_operation_alert_delivery(
            OperationAlertDeliveryWorker::new(summary_store, sink.clone(), checkpoint),
            OperationAlertDeliverySchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        )
        .run_until_shutdown(async {})
        .await
        .expect("runtime should run operation alert worker and stop on shutdown");

    let alert_report = report
        .operation_alert_delivery
        .expect("runtime should include operation alert scheduler report");
    assert_eq!(alert_report.runs, 1);
    assert_eq!(
        alert_report
            .last_report
            .expect("alert worker should retain last report")
            .newly_delivered,
        1
    );
    assert_eq!(sink.alerts().len(), 1);
}

#[tokio::test]
async fn worker_runtime_rejects_invalid_schedule_before_spawning_workers() {
    let temp = temp_test_dir("runtime-invalid-schedule");
    let sink = Arc::new(InMemoryEventSink::default());
    let gc_worker = JournalGcWorker::new(
        JournalGc::new(temp.join("journal")),
        RetentionPolicy::default(),
        sink.clone(),
    );

    let err = UnderlayWorkerRuntime::new()
        .with_journal_gc(
            gc_worker,
            JournalGcSchedule {
                interval_secs: 0,
                run_immediately: true,
            },
        )
        .run_until_shutdown(async {})
        .await
        .expect_err("invalid runtime schedule should fail closed");
    let message = format!("{err}");

    assert!(
        message.contains("interval_secs"),
        "unexpected runtime validation error: {message}"
    );
    assert!(
        sink.events().is_empty(),
        "runtime must validate schedules before emitting worker events"
    );

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn worker_runtime_keeps_other_workers_running_when_one_worker_fails() {
    let temp = temp_test_dir("runtime-isolates-worker-error");
    let failing_journal_root = temp.join("journal-root-is-file");
    fs::create_dir_all(&temp).expect("temp root should be created");
    fs::write(&failing_journal_root, b"not a directory")
        .expect("failing journal root marker should be written");

    let sink = Arc::new(InMemoryEventSink::default());
    let gc_worker = JournalGcWorker::new(
        JournalGc::new(&failing_journal_root),
        RetentionPolicy::default(),
        sink.clone(),
    );
    let drift_worker = DriftAuditWorker::new(
        DriftAuditor::new(vec![DriftAuditSnapshot {
            expected: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
            observed: shadow_state("leaf-a", vec![], vec![]),
        }]),
        sink,
    );

    let report = UnderlayWorkerRuntime::new()
        .with_journal_gc(
            gc_worker,
            JournalGcSchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        )
        .with_drift_audit(
            drift_worker,
            DriftAuditSchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        )
        .run_until_shutdown(async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        })
        .await
        .expect("runtime should isolate worker errors");

    assert_eq!(report.worker_errors.len(), 1);
    assert!(report.worker_errors[0].contains("journal_gc"));
    assert_eq!(
        report
            .drift_audit
            .expect("healthy drift worker should still report")
            .runs,
        1
    );

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn journal_gc_skips_malformed_journal_file_and_continues() {
    let temp = temp_test_dir("gc-skips-malformed-journal");
    let journal_root = temp.join("journal");
    let journal = JsonFileTxJournalStore::new(&journal_root);
    journal
        .put(&journal_record("tx-old", TxPhase::Committed, 100))
        .expect("old terminal journal should be stored");
    fs::write(journal_root.join("bad.json"), b"{not-json")
        .expect("malformed journal file should be written");

    let report = JournalGc::new(&journal_root)
        .with_now_unix_secs(100 + 31 * 24 * 60 * 60)
        .run_once(RetentionPolicy::default())
        .await
        .expect("GC should skip one malformed file and continue");

    assert_eq!(report.journals_deleted, 1);
    assert_eq!(report.journal_deleted_tx_ids, vec!["tx-old".to_string()]);
    assert_eq!(report.journals_failed, 1);
    assert_eq!(report.failed_journal_refs, vec!["bad.json".to_string()]);

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn drift_audit_skips_failed_device_observation_and_continues() {
    let expected_store = Arc::new(aria_underlay::state::InMemoryShadowStateStore::default());
    expected_store
        .put(shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]))
        .expect("leaf-a expected state should be stored");
    expected_store
        .put(shadow_state("leaf-b", vec![vlan(200, "prod")], vec![]))
        .expect("leaf-b expected state should be stored");
    let observed_source = Arc::new(PartiallyFailingObservationSource);
    let auditor = DriftAuditor::from_source(expected_store, observed_source);

    let summary = auditor
        .run_once_with_summary()
        .await
        .expect("drift audit should continue after one device observation error");

    assert_eq!(summary.audited_devices, 2);
    assert_eq!(summary.failed_devices, vec![DeviceId("leaf-b".into())]);
    assert_eq!(summary.drifted_devices, vec![DeviceId("leaf-a".into())]);
}

fn recovery_event(request_id: &str, in_doubt: usize) -> UnderlayEvent {
    UnderlayEvent::recovery_completed(
        request_id,
        format!("trace-{request_id}"),
        &RecoveryReport {
            recovered: 0,
            in_doubt,
            pending: 0,
            tx_ids: vec![format!("tx-{request_id}")],
            decisions: Vec::new(),
        },
    )
}

#[derive(Debug)]
struct PartiallyFailingObservationSource;

#[async_trait]
impl DriftObservationSource for PartiallyFailingObservationSource {
    async fn get_observed_state(&self, device_id: &DeviceId) -> UnderlayResult<DeviceShadowState> {
        if device_id.0 == "leaf-b" {
            return Err(UnderlayError::AdapterTransport(
                "leaf-b observation failed".into(),
            ));
        }
        Ok(shadow_state(&device_id.0, vec![], vec![]))
    }
}

fn journal_record(tx_id: &str, phase: TxPhase, updated_at_unix_secs: u64) -> TxJournalRecord {
    TxJournalRecord {
        tx_id: tx_id.into(),
        request_id: format!("req-{tx_id}"),
        trace_id: format!("trace-{tx_id}"),
        phase,
        devices: vec![DeviceId("leaf-a".into())],
        desired_states: Vec::new(),
        change_sets: Vec::new(),
        strategy: None,
        error_code: None,
        error_message: None,
        error_history: Vec::new(),
        manual_resolution: None,
        created_at_unix_secs: updated_at_unix_secs,
        updated_at_unix_secs,
    }
}

fn shadow_state(
    device_id: &str,
    vlans: Vec<VlanConfig>,
    interfaces: Vec<InterfaceConfig>,
) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId(device_id.into()),
        revision: 1,
        vlans: vlans
            .into_iter()
            .map(|vlan| (vlan.vlan_id, vlan))
            .collect::<BTreeMap<_, _>>(),
        interfaces: interfaces
            .into_iter()
            .map(|interface| (interface.name.clone(), interface))
            .collect::<BTreeMap<_, _>>(),
        warnings: Vec::new(),
    }
}

fn vlan(vlan_id: u16, name: &str) -> VlanConfig {
    VlanConfig {
        vlan_id,
        name: Some(name.into()),
        description: None,
    }
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-worker-runtime-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
