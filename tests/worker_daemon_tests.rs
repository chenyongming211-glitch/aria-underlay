use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use aria_underlay::model::{DeviceId, InterfaceConfig, VlanConfig};
use aria_underlay::state::{DeviceShadowState, JsonFileShadowStateStore, ShadowStateStore};
use aria_underlay::telemetry::{
    JsonFileOperationAlertSink, JsonFileOperationAuditStore, JsonFileOperationSummaryStore,
    OperationAuditRetentionPolicy, OperationSummaryRetentionPolicy, UnderlayEvent,
};
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::worker::daemon::{
    DriftAuditDaemonConfig, JournalGcDaemonConfig, OperationAlertDaemonConfig,
    OperationAuditDaemonConfig, OperationSummaryDaemonConfig, UnderlayWorkerDaemon,
    UnderlayWorkerDaemonConfig, WorkerConfigReloadStatus, WorkerReloadCheckpoint,
    WorkerReloadDaemonConfig, WorkerScheduleConfig,
};
use aria_underlay::worker::gc::RetentionPolicy;
use tokio::sync::watch;

#[test]
fn checked_in_worker_daemon_sample_config_parses() {
    let config = UnderlayWorkerDaemonConfig::from_path(
        "docs/examples/underlay-worker-daemon.local.json",
    )
    .expect("checked-in worker daemon sample config should parse");

    assert!(config.operation_summary.is_some());
    assert!(config.operation_audit.is_some());
    assert!(config.operation_alert.is_some());
    assert!(config.journal_gc.is_some());
    assert!(config.drift_audit.is_some());
}

#[tokio::test]
async fn daemon_config_wires_gc_drift_and_persistent_operation_summaries() {
    let temp = temp_test_dir("daemon-wires-workers");
    let journal_root = temp.join("journal");
    let expected_shadow_root = temp.join("expected-shadow");
    let observed_shadow_root = temp.join("observed-shadow");
    let operation_summary_path = temp.join("ops").join("summaries.jsonl");
    let operation_audit_path = temp.join("ops").join("audit.jsonl");

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
        reload: None,
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: operation_summary_path.clone(),
            retention: OperationSummaryRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig::default(),
        }),
        operation_audit: Some(OperationAuditDaemonConfig {
            path: operation_audit_path.clone(),
            retention: OperationAuditRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig::default(),
        }),
        operation_alert: None,
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
    let audit_records = JsonFileOperationAuditStore::new(&operation_audit_path)
        .list()
        .expect("operation audit records should be persisted by daemon workers");
    let mut audit_actions = audit_records
        .iter()
        .map(|record| record.action.as_str())
        .collect::<Vec<_>>();
    audit_actions.sort();
    assert_eq!(
        audit_actions,
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
        reload: None,
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
        operation_audit: None,
        operation_alert: None,
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
async fn daemon_config_wires_operation_audit_retention_worker() {
    let temp = temp_test_dir("daemon-operation-audit-retention");
    let operation_audit_path = temp.join("ops").join("audit.jsonl");
    let store = JsonFileOperationAuditStore::new(&operation_audit_path);
    for index in 1..=3 {
        store
            .record_event(&recovery_event(index))
            .expect("operation audit should persist before daemon compaction");
    }

    let config = UnderlayWorkerDaemonConfig {
        reload: None,
        operation_summary: None,
        operation_audit: Some(OperationAuditDaemonConfig {
            path: operation_audit_path.clone(),
            retention: OperationAuditRetentionPolicy {
                max_records: Some(1),
                max_bytes: None,
                max_rotated_files: 1,
            },
            retention_schedule: WorkerScheduleConfig {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        }),
        operation_alert: None,
        journal_gc: None,
        drift_audit: None,
    };

    let report = UnderlayWorkerDaemon::from_config(config)
        .expect("daemon config should build operation audit retention runtime")
        .run_until_shutdown(async {})
        .await
        .expect("daemon audit retention worker should stop cleanly on shutdown");

    assert_eq!(
        report
            .operation_audit_compaction
            .as_ref()
            .expect("operation audit compaction report should be present")
            .runs,
        1
    );
    let records = JsonFileOperationAuditStore::new(&operation_audit_path)
        .list()
        .expect("compacted audit records should be readable");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].request_id, "req-3");

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn daemon_config_wires_operation_alert_delivery_worker() {
    let temp = temp_test_dir("daemon-operation-alert-delivery");
    let operation_summary_path = temp.join("ops").join("summaries.jsonl");
    let alert_path = temp.join("alerts").join("alerts.jsonl");
    let checkpoint_path = temp.join("alerts").join("checkpoint.json");
    let summary_store = JsonFileOperationSummaryStore::new(&operation_summary_path);
    summary_store
        .record_event(&attention_recovery_event(1))
        .expect("attention-required operation summary should persist before alert delivery");
    summary_store
        .record_event(&attention_recovery_event(2))
        .expect("second attention-required operation summary should persist before alert delivery");

    let config = UnderlayWorkerDaemonConfig {
        reload: None,
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: operation_summary_path.clone(),
            retention: OperationSummaryRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig::default(),
        }),
        operation_audit: None,
        operation_alert: Some(OperationAlertDaemonConfig {
            path: alert_path.clone(),
            checkpoint_path: checkpoint_path.clone(),
            schedule: WorkerScheduleConfig {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        }),
        journal_gc: None,
        drift_audit: None,
    };

    let report = UnderlayWorkerDaemon::from_config(config)
        .expect("daemon config should build operation alert runtime")
        .run_until_shutdown(async {})
        .await
        .expect("daemon alert worker should stop cleanly on shutdown");

    assert_eq!(
        report
            .operation_alert_delivery
            .as_ref()
            .expect("operation alert report should be present")
            .runs,
        1
    );
    let alerts = JsonFileOperationAlertSink::new(&alert_path)
        .list()
        .expect("persisted alerts should be readable");
    assert_eq!(alerts.len(), 2);
    assert!(checkpoint_path.exists());

    let second_config = UnderlayWorkerDaemonConfig {
        reload: None,
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: operation_summary_path.clone(),
            retention: OperationSummaryRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig::default(),
        }),
        operation_audit: None,
        operation_alert: Some(OperationAlertDaemonConfig {
            path: alert_path.clone(),
            checkpoint_path: checkpoint_path.clone(),
            schedule: WorkerScheduleConfig {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
        }),
        journal_gc: None,
        drift_audit: None,
    };
    UnderlayWorkerDaemon::from_config(second_config)
        .expect("daemon config should rebuild operation alert runtime")
        .run_until_shutdown(async {})
        .await
        .expect("second daemon alert worker run should stop cleanly");
    let alerts_after_restart = JsonFileOperationAlertSink::new(&alert_path)
        .list()
        .expect("persisted alerts should be readable after restart");
    assert_eq!(alerts_after_restart.len(), 2);

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn daemon_config_file_rejects_invalid_worker_schedule_before_start() {
    let temp = temp_test_dir("daemon-invalid-schedule");
    let config_path = temp.join("worker.json");
    let config_json = format!(
        r#"{{
            "operation_summary": {{"path": "{}"}},
            "operation_alert": null,
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

#[tokio::test]
async fn reloadable_daemon_applies_valid_config_change_and_records_checkpoint() {
    let temp = temp_test_dir("daemon-reload-valid");
    let config_path = temp.join("worker.json");
    let checkpoint_path = temp.join("ops").join("worker-reload-checkpoint.json");
    let mut config = reloadable_worker_config(&temp, 3_600);
    fs::create_dir_all(&temp).expect("temp config dir should be created");
    config
        .write_to_path(&config_path)
        .expect("initial reloadable worker config should be written");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon_task = tokio::spawn({
        let config_path = config_path.clone();
        async move {
            UnderlayWorkerDaemon::run_config_path_until_shutdown(
                config_path,
                wait_for_watch_shutdown(shutdown_rx),
            )
            .await
        }
    });

    let initial_checkpoint = wait_for_reload_checkpoint(&checkpoint_path, |checkpoint| {
        checkpoint.status == WorkerConfigReloadStatus::Started
    })
    .await;
    assert_eq!(initial_checkpoint.generation, 1);

    config
        .operation_summary
        .as_mut()
        .expect("operation summary should exist")
        .retention_schedule
        .interval_secs = 900;
    config
        .write_to_path(&config_path)
        .expect("updated reloadable worker config should be written");

    let applied_checkpoint = wait_for_reload_checkpoint(&checkpoint_path, |checkpoint| {
        checkpoint.status == WorkerConfigReloadStatus::Applied && checkpoint.generation == 2
    })
    .await;
    assert_eq!(applied_checkpoint.error, None);

    shutdown_tx
        .send(true)
        .expect("daemon shutdown signal should be sent");
    tokio::time::timeout(Duration::from_secs(5), daemon_task)
        .await
        .expect("daemon should stop after shutdown")
        .expect("daemon task should join")
        .expect("reloadable daemon should stop cleanly");

    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn reloadable_daemon_rejects_invalid_config_change_without_replacing_runtime() {
    let temp = temp_test_dir("daemon-reload-invalid");
    let config_path = temp.join("worker.json");
    let checkpoint_path = temp.join("ops").join("worker-reload-checkpoint.json");
    let mut config = reloadable_worker_config(&temp, 3_600);
    fs::create_dir_all(&temp).expect("temp config dir should be created");
    config
        .write_to_path(&config_path)
        .expect("initial reloadable worker config should be written");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let daemon_task = tokio::spawn({
        let config_path = config_path.clone();
        async move {
            UnderlayWorkerDaemon::run_config_path_until_shutdown(
                config_path,
                wait_for_watch_shutdown(shutdown_rx),
            )
            .await
        }
    });

    wait_for_reload_checkpoint(&checkpoint_path, |checkpoint| {
        checkpoint.status == WorkerConfigReloadStatus::Started
    })
    .await;

    config
        .operation_summary
        .as_mut()
        .expect("operation summary should exist")
        .retention_schedule
        .interval_secs = 0;
    config
        .write_to_path(&config_path)
        .expect("invalid reload candidate should be written");

    let rejected_checkpoint = wait_for_reload_checkpoint(&checkpoint_path, |checkpoint| {
        checkpoint.status == WorkerConfigReloadStatus::Rejected
    })
    .await;
    assert_eq!(rejected_checkpoint.generation, 1);
    assert!(
        rejected_checkpoint
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("interval_secs"),
        "invalid reload checkpoint should include schedule error: {rejected_checkpoint:#?}"
    );

    shutdown_tx
        .send(true)
        .expect("daemon shutdown signal should be sent");
    tokio::time::timeout(Duration::from_secs(5), daemon_task)
        .await
        .expect("daemon should stop after rejected reload")
        .expect("daemon task should join")
        .expect("reloadable daemon should keep old runtime and stop cleanly");

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

fn attention_recovery_event(index: usize) -> UnderlayEvent {
    UnderlayEvent::recovery_completed(
        format!("req-alert-{index}"),
        format!("trace-alert-{index}"),
        &aria_underlay::tx::recovery::RecoveryReport {
            recovered: 0,
            in_doubt: index,
            pending: 0,
            tx_ids: vec![format!("tx-alert-{index}")],
            decisions: Vec::new(),
        },
    )
}

fn reloadable_worker_config(
    temp: &std::path::Path,
    retention_interval_secs: u64,
) -> UnderlayWorkerDaemonConfig {
    UnderlayWorkerDaemonConfig {
        reload: Some(WorkerReloadDaemonConfig {
            enabled: true,
            poll_interval_secs: 1,
            checkpoint_path: Some(temp.join("ops").join("worker-reload-checkpoint.json")),
        }),
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: temp.join("ops").join("summaries.jsonl"),
            retention: OperationSummaryRetentionPolicy {
                max_records: Some(10_000),
                max_bytes: None,
                max_rotated_files: 5,
            },
            retention_schedule: WorkerScheduleConfig {
                interval_secs: retention_interval_secs,
                run_immediately: false,
            },
        }),
        operation_audit: None,
        operation_alert: None,
        journal_gc: None,
        drift_audit: None,
    }
}

async fn wait_for_watch_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

async fn wait_for_reload_checkpoint(
    checkpoint_path: &std::path::Path,
    predicate: impl Fn(&WorkerReloadCheckpoint) -> bool,
) -> WorkerReloadCheckpoint {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(payload) = fs::read(checkpoint_path) {
            let checkpoint: WorkerReloadCheckpoint =
                serde_json::from_slice(&payload).expect("checkpoint should be valid JSON");
            if predicate(&checkpoint) {
                return checkpoint;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for reload checkpoint at {:?}",
            checkpoint_path
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-worker-daemon-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
