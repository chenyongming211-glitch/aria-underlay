use std::fs;
use std::sync::Arc;

use aria_underlay::api::worker_config_admin::{
    ChangeSummaryRetentionRequest, ChangeWorkerScheduleRequest, WorkerConfigAdminManager,
    WorkerScheduleTarget,
};
use aria_underlay::authz::{RbacRole, StaticAuthorizationPolicy};
use aria_underlay::telemetry::{
    InMemoryProductAuditStore, OperationSummaryRetentionPolicy, ProductAuditRecord,
    ProductAuditStore,
};
use aria_underlay::worker::daemon::{
    DriftAuditDaemonConfig, JournalGcDaemonConfig, OperationAlertDaemonConfig,
    OperationSummaryDaemonConfig, UnderlayWorkerDaemonConfig, WorkerScheduleConfig,
};
use aria_underlay::worker::gc::RetentionPolicy;
use aria_underlay::{UnderlayError, UnderlayResult};

#[test]
fn admin_changes_summary_retention_after_product_audit() {
    let temp = temp_test_dir("summary-retention-admin");
    let config_path = temp.join("worker.json");
    worker_config(&temp)
        .write_to_path(&config_path)
        .expect("worker config should be written");
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let manager = manager_with_role("admin-a", RbacRole::Admin, audit_store.clone());

    let response = manager
        .change_summary_retention(ChangeSummaryRetentionRequest {
            request_id: "req-retention".into(),
            trace_id: Some("trace-retention".into()),
            config_path: config_path.clone(),
            operator: "admin-a".into(),
            reason: "tighten local operation summary retention".into(),
            retention: OperationSummaryRetentionPolicy {
                max_records: Some(25),
                max_bytes: Some(4096),
                max_rotated_files: 3,
            },
        })
        .expect("admin should change operation summary retention");

    assert_eq!(response.target, "operation_summary");
    assert!(response.changed);
    let updated = UnderlayWorkerDaemonConfig::from_path(&config_path)
        .expect("updated worker config should parse");
    let retention = updated
        .operation_summary
        .expect("operation_summary section should remain present")
        .retention;
    assert_eq!(retention.max_records, Some(25));
    assert_eq!(retention.max_bytes, Some(4096));
    assert_eq!(retention.max_rotated_files, 3);

    let records = audit_store.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].action, "daemon.retention_change_requested");
    assert_eq!(records[0].result, "authorized");
    assert_eq!(records[0].operator_id.as_deref(), Some("admin-a"));
    assert_eq!(records[0].role, Some(RbacRole::Admin));
    assert_eq!(
        records[0].fields.get("target").map(String::as_str),
        Some("operation_summary")
    );

    fs::remove_dir_all(temp).ok();
}

#[test]
fn non_admin_cannot_change_worker_schedule_and_config_stays_unchanged() {
    let temp = temp_test_dir("schedule-denied");
    let config_path = temp.join("worker.json");
    worker_config(&temp)
        .write_to_path(&config_path)
        .expect("worker config should be written");
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let manager = manager_with_role("viewer-a", RbacRole::Viewer, audit_store.clone());

    let err = manager
        .change_worker_schedule(schedule_request(
            &config_path,
            "viewer-a",
            WorkerScheduleTarget::JournalGc,
            120,
        ))
        .expect_err("viewer should not change daemon schedule");

    assert!(matches!(err, UnderlayError::AuthorizationDenied(_)));
    let config = UnderlayWorkerDaemonConfig::from_path(&config_path)
        .expect("worker config should still parse");
    assert_eq!(
        config.journal_gc.expect("journal_gc should exist").schedule.interval_secs,
        60
    );
    assert!(audit_store.records().is_empty());

    fs::remove_dir_all(temp).ok();
}

#[test]
fn product_audit_failure_blocks_worker_config_mutation() {
    let temp = temp_test_dir("audit-failure");
    let config_path = temp.join("worker.json");
    worker_config(&temp)
        .write_to_path(&config_path)
        .expect("worker config should be written");
    let manager = WorkerConfigAdminManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role("admin-a", RbacRole::Admin)),
        Arc::new(FailingProductAuditStore),
    );

    let err = manager
        .change_worker_schedule(schedule_request(
            &config_path,
            "admin-a",
            WorkerScheduleTarget::OperationAlert,
            300,
        ))
        .expect_err("audit failure should fail closed");

    assert!(matches!(err, UnderlayError::ProductAuditWriteFailed(_)));
    let config = UnderlayWorkerDaemonConfig::from_path(&config_path)
        .expect("worker config should still parse");
    assert_eq!(
        config
            .operation_alert
            .expect("operation_alert should exist")
            .schedule
            .interval_secs,
        60
    );

    fs::remove_dir_all(temp).ok();
}

#[test]
fn invalid_schedule_rejects_before_audit_and_config_mutation() {
    let temp = temp_test_dir("invalid-schedule");
    let config_path = temp.join("worker.json");
    worker_config(&temp)
        .write_to_path(&config_path)
        .expect("worker config should be written");
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let manager = manager_with_role("admin-a", RbacRole::Admin, audit_store.clone());

    let err = manager
        .change_worker_schedule(schedule_request(
            &config_path,
            "admin-a",
            WorkerScheduleTarget::DriftAudit,
            0,
        ))
        .expect_err("zero interval should be rejected before audit");

    assert!(matches!(err, UnderlayError::InvalidIntent(_)));
    assert!(audit_store.records().is_empty());
    let config = UnderlayWorkerDaemonConfig::from_path(&config_path)
        .expect("worker config should still parse");
    assert_eq!(
        config
            .drift_audit
            .expect("drift_audit should exist")
            .schedule
            .interval_secs,
        60
    );

    fs::remove_dir_all(temp).ok();
}

#[derive(Debug)]
struct FailingProductAuditStore;

impl ProductAuditStore for FailingProductAuditStore {
    fn append(&self, _record: ProductAuditRecord) -> UnderlayResult<()> {
        Err(UnderlayError::ProductAuditWriteFailed(
            "simulated product audit write failure".into(),
        ))
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        Ok(Vec::new())
    }
}

fn manager_with_role(
    operator: &str,
    role: RbacRole,
    audit_store: Arc<InMemoryProductAuditStore>,
) -> WorkerConfigAdminManager {
    WorkerConfigAdminManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role(operator, role)),
        audit_store,
    )
}

fn schedule_request(
    config_path: &std::path::Path,
    operator: &str,
    target: WorkerScheduleTarget,
    interval_secs: u64,
) -> ChangeWorkerScheduleRequest {
    ChangeWorkerScheduleRequest {
        request_id: format!("req-schedule-{interval_secs}"),
        trace_id: Some(format!("trace-schedule-{interval_secs}")),
        config_path: config_path.into(),
        operator: operator.into(),
        reason: "change worker schedule".into(),
        target,
        schedule: WorkerScheduleConfig {
            interval_secs,
            run_immediately: false,
        },
    }
}

fn worker_config(temp: &std::path::Path) -> UnderlayWorkerDaemonConfig {
    UnderlayWorkerDaemonConfig {
        reload: None,
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: temp.join("ops").join("summaries.jsonl"),
            retention: OperationSummaryRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
        operation_alert: Some(OperationAlertDaemonConfig {
            path: temp.join("ops").join("alerts.jsonl"),
            checkpoint_path: temp.join("ops").join("alert-checkpoint.json"),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
        journal_gc: Some(JournalGcDaemonConfig {
            journal_root: temp.join("journal"),
            artifact_root: Some(temp.join("artifacts")),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
            retention: RetentionPolicy::default(),
        }),
        drift_audit: Some(DriftAuditDaemonConfig {
            expected_shadow_root: temp.join("expected-shadow"),
            observed_shadow_root: temp.join("observed-shadow"),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
    }
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-worker-config-admin-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
