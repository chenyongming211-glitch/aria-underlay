use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use aria_underlay::model::{DeviceId, InterfaceConfig, VlanConfig};
use aria_underlay::state::DeviceShadowState;
use aria_underlay::telemetry::{InMemoryEventSink, UnderlayEventKind};
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::worker::drift_auditor::{
    DriftAuditSchedule, DriftAuditSnapshot, DriftAuditWorker, DriftAuditor,
};
use aria_underlay::worker::gc::{
    JournalGc, JournalGcSchedule, JournalGcWorker, RetentionPolicy,
};
use aria_underlay::worker::runtime::UnderlayWorkerRuntime;

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
