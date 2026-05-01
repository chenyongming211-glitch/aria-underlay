use std::collections::BTreeMap;
use std::fs;

use aria_underlay::model::{DeviceId, InterfaceConfig, VlanConfig};
use aria_underlay::state::{DeviceShadowState, JsonFileShadowStateStore, ShadowStateStore};
use aria_underlay::telemetry::{
    JsonFileOperationSummaryStore, OperationSummaryRetentionPolicy, UnderlayEvent,
};
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::worker::daemon::{
    DriftAuditDaemonConfig, JournalGcDaemonConfig, OperationSummaryDaemonConfig,
    UnderlayWorkerDaemon, UnderlayWorkerDaemonConfig, WorkerScheduleConfig,
};
use aria_underlay::worker::gc::RetentionPolicy;

#[tokio::test]
async fn daemon_config_wires_gc_drift_and_persistent_operation_summaries() {
    let temp = temp_test_dir("daemon-wires-workers");
    let journal_root = temp.join("journal");
    let expected_shadow_root = temp.join("expected-shadow");
    let observed_shadow_root = temp.join("observed-shadow");
    let operation_summary_path = temp.join("ops").join("summaries.jsonl");

    JsonFileTxJournalStore::new(&journal_root)
        .put(&journal_record("tx-old", TxPhase::Committed, 100))
        .expect("old terminal journal should be stored");
    JsonFileShadowStateStore::new(&expected_shadow_root)
        .put(shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]))
        .expect("expected shadow should be stored");
    JsonFileShadowStateStore::new(&observed_shadow_root)
        .put(shadow_state("leaf-a", vec![], vec![]))
        .expect("observed shadow should be stored");

    let config = UnderlayWorkerDaemonConfig {
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: operation_summary_path.clone(),
            retention: OperationSummaryRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig::default(),
        }),
        journal_gc: Some(JournalGcDaemonConfig {
            journal_root: journal_root.clone(),
            artifact_root: None,
            schedule: WorkerScheduleConfig {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
            retention: RetentionPolicy {
                committed_journal_retention_days: 30,
                rolled_back_journal_retention_days: 30,
                failed_journal_retention_days: 90,
                rollback_artifact_retention_days: 30,
                max_artifacts_per_device: 50,
            },
        }),
        drift_audit: Some(DriftAuditDaemonConfig {
            expected_shadow_root: expected_shadow_root.clone(),
            observed_shadow_root: observed_shadow_root.clone(),
            schedule: WorkerScheduleConfig {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        }),
    };

    let report = UnderlayWorkerDaemon::from_config(config)
        .expect("daemon config should build runtime")
        .run_until_shutdown(async {})
        .await
        .expect("daemon runtime should stop cleanly on shutdown");

    assert_eq!(
        report
            .journal_gc
            .as_ref()
            .expect("GC report should be present")
            .runs,
        1
    );
    assert_eq!(
        report
            .drift_audit
            .as_ref()
            .expect("drift report should be present")
            .runs,
        1
    );

    let summaries = JsonFileOperationSummaryStore::new(&operation_summary_path)
        .list()
        .expect("operation summaries should be persisted by daemon workers");
    let mut actions = summaries
        .iter()
        .map(|summary| summary.action.as_str())
        .collect::<Vec<_>>();
    actions.sort();
    assert_eq!(
        actions,
        vec![
            "drift.audit_completed",
            "drift.detected",
            "journal.gc_completed"
        ]
    );

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn daemon_config_wires_operation_summary_retention_worker() {
    let temp = temp_test_dir("daemon-operation-summary-retention");
    let operation_summary_path = temp.join("ops").join("summaries.jsonl");
    let store = JsonFileOperationSummaryStore::new(&operation_summary_path);
    for index in 1..=3 {
        store
            .record_event(&recovery_event(index))
            .expect("operation summary should persist before daemon compaction");
    }

    let config = UnderlayWorkerDaemonConfig {
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: operation_summary_path.clone(),
            retention: OperationSummaryRetentionPolicy {
                max_records: Some(1),
                max_bytes: None,
                max_rotated_files: 1,
            },
            retention_schedule: WorkerScheduleConfig {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        }),
        journal_gc: None,
        drift_audit: None,
    };

    let report = UnderlayWorkerDaemon::from_config(config)
        .expect("daemon config should build operation summary retention runtime")
        .run_until_shutdown(async {})
        .await
        .expect("daemon retention worker should stop cleanly on shutdown");

    assert_eq!(
        report
            .operation_summary_compaction
            .as_ref()
            .expect("operation summary compaction report should be present")
            .runs,
        1
    );
    let summaries = JsonFileOperationSummaryStore::new(&operation_summary_path)
        .list()
        .expect("compacted summaries should be readable");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].request_id, "req-3");

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn daemon_config_file_rejects_invalid_worker_schedule_before_start() {
    let temp = temp_test_dir("daemon-invalid-schedule");
    let config_path = temp.join("worker.json");
    let config_json = format!(
        r#"{{
            "operation_summary": {{"path": "{}"}},
            "journal_gc": {{
                "journal_root": "{}",
                "schedule": {{"interval_secs": 0, "run_immediately": true}}
            }}
        }}"#,
        temp.join("ops").join("summaries.jsonl").display(),
        temp.join("journal").display()
    );
    fs::create_dir_all(&temp).expect("temp config dir should be created");
    fs::write(&config_path, config_json).expect("worker config should be written");

    let config = UnderlayWorkerDaemonConfig::from_path(&config_path)
        .expect("JSON daemon config should parse");
    let err = UnderlayWorkerDaemon::from_config(config)
        .expect("daemon construction can defer runtime validation")
        .run_until_shutdown(async {})
        .await
        .expect_err("invalid schedule should fail closed");
    let message = format!("{err}");

    assert!(
        message.contains("interval_secs"),
        "unexpected daemon validation error: {message}"
    );
    assert!(
        !temp.join("ops").join("summaries.jsonl").exists(),
        "daemon should validate schedule before worker events are persisted"
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

fn recovery_event(index: usize) -> UnderlayEvent {
    UnderlayEvent::recovery_completed(
        format!("req-{index}"),
        format!("trace-{index}"),
        &aria_underlay::tx::recovery::RecoveryReport {
            recovered: index,
            in_doubt: 0,
            pending: 0,
            tx_ids: vec![format!("tx-{index}")],
            decisions: Vec::new(),
        },
    )
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-worker-daemon-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
