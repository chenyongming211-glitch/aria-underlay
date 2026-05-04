use std::fs;

use aria_underlay::telemetry::OperationSummaryRetentionPolicy;
use aria_underlay::worker::daemon::{
    DriftAuditDaemonConfig, JournalGcDaemonConfig, OperationAlertDaemonConfig,
    OperationSummaryDaemonConfig, UnderlayWorkerDaemonConfig, WorkerReloadDaemonConfig,
    WorkerScheduleConfig,
};
use aria_underlay::worker::deployment::WorkerDeploymentPreflight;
use aria_underlay::worker::gc::RetentionPolicy;

#[test]
fn checked_in_worker_deployment_samples_are_consistent() {
    let config =
        UnderlayWorkerDaemonConfig::from_path("docs/examples/underlay-worker-daemon.production.json")
            .expect("checked-in production worker config should parse");
    let report = WorkerDeploymentPreflight::new().check_config(&config);
    assert!(
        report.valid,
        "production sample should pass semantic preflight: {report:#?}"
    );

    let systemd_unit =
        fs::read_to_string("docs/examples/systemd/aria-underlay-worker.service")
            .expect("checked-in systemd unit should exist");
    assert!(systemd_unit.contains("User=aria-underlay"));
    assert!(systemd_unit.contains(
        "ExecStartPre=/usr/local/bin/aria-underlay-ops check-worker-config --worker-config-path /etc/aria-underlay/worker.json --strict-paths"
    ));
    assert!(systemd_unit
        .contains("ExecStart=/usr/local/bin/aria-underlay-worker /etc/aria-underlay/worker.json"));
    assert!(systemd_unit
        .contains("ReadWritePaths=/var/lib/aria-underlay /var/log/aria-underlay /run/aria-underlay"));

    let tmpfiles = fs::read_to_string("docs/examples/tmpfiles.d/aria-underlay.conf")
        .expect("checked-in tmpfiles.d sample should exist");
    assert!(tmpfiles.contains("/var/lib/aria-underlay/ops"));
    assert!(tmpfiles.contains("/var/lib/aria-underlay/journal"));
    assert!(tmpfiles.contains("/var/lib/aria-underlay/shadow/expected"));
    assert!(tmpfiles.contains("/var/lib/aria-underlay/shadow/observed"));
}

#[test]
fn preflight_accepts_valid_config_when_strict_paths_exist() {
    let temp = temp_test_dir("strict-valid");
    let config = worker_config(&temp);
    create_worker_dirs(&config);

    let report = WorkerDeploymentPreflight::new()
        .strict_paths(true)
        .check_config(&config);

    assert!(report.valid, "strict preflight should pass: {report:#?}");
    assert!(report.errors.is_empty());
    assert!(!report.checked_paths.is_empty());
    assert!(
        report
            .checked_paths
            .iter()
            .any(|check| check.kind == "operation_summary.path.parent")
    );
    assert!(
        report
            .checked_paths
            .iter()
            .filter_map(|check| check.writable)
            .all(|writable| writable),
        "all strict path write probes should be writable: {report:#?}"
    );

    fs::remove_dir_all(temp).ok();
}

#[test]
fn preflight_rejects_invalid_schedule_without_starting_workers() {
    let temp = temp_test_dir("invalid-schedule");
    let mut config = worker_config(&temp);
    config
        .journal_gc
        .as_mut()
        .expect("journal_gc should exist")
        .schedule
        .interval_secs = 0;

    let report = WorkerDeploymentPreflight::new().check_config(&config);

    assert!(!report.valid);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("journal_gc.schedule.interval_secs")),
        "invalid schedule should be reported: {report:#?}"
    );
    assert!(
        !temp.join("ops").join("summaries.jsonl").exists(),
        "preflight must not start daemon workers or write summaries"
    );

    fs::remove_dir_all(temp).ok();
}

#[test]
fn preflight_rejects_invalid_reload_config_without_starting_workers() {
    let temp = temp_test_dir("invalid-reload");
    let mut config = worker_config(&temp);
    config.reload = Some(WorkerReloadDaemonConfig {
        enabled: true,
        poll_interval_secs: 0,
        checkpoint_path: None,
    });

    let report = WorkerDeploymentPreflight::new().check_config(&config);

    assert!(!report.valid);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("reload.poll_interval_secs")),
        "reload poll interval should be reported: {report:#?}"
    );
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("reload.checkpoint_path")),
        "reload checkpoint path should be reported: {report:#?}"
    );
    assert!(
        !temp.join("ops").join("summaries.jsonl").exists(),
        "preflight must not start daemon workers or write summaries"
    );

    fs::remove_dir_all(temp).ok();
}

#[test]
fn preflight_strict_paths_rejects_missing_directory() {
    let temp = temp_test_dir("missing-dir");
    let config = worker_config(&temp);

    let report = WorkerDeploymentPreflight::new()
        .strict_paths(true)
        .check_config(&config);

    assert!(!report.valid);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("missing required directory")),
        "missing directory should be reported: {report:#?}"
    );

    fs::remove_dir_all(temp).ok();
}

fn worker_config(temp: &std::path::Path) -> UnderlayWorkerDaemonConfig {
    UnderlayWorkerDaemonConfig {
        reload: None,
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: temp.join("ops").join("summaries.jsonl"),
            retention: OperationSummaryRetentionPolicy {
                max_records: Some(10_000),
                max_bytes: Some(10 * 1024 * 1024),
                max_rotated_files: 5,
            },
            retention_schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
        operation_audit: None,
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
            expected_shadow_root: temp.join("shadow").join("expected"),
            observed_shadow_root: temp.join("shadow").join("observed"),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
    }
}

fn create_worker_dirs(config: &UnderlayWorkerDaemonConfig) {
    if let Some(operation_summary) = &config.operation_summary {
        fs::create_dir_all(operation_summary.path.parent().expect("summary parent"))
            .expect("summary parent should be created");
    }
    if let Some(operation_audit) = &config.operation_audit {
        fs::create_dir_all(operation_audit.path.parent().expect("audit parent"))
            .expect("audit parent should be created");
    }
    if let Some(operation_alert) = &config.operation_alert {
        fs::create_dir_all(operation_alert.path.parent().expect("alert parent"))
            .expect("alert parent should be created");
        fs::create_dir_all(
            operation_alert
                .checkpoint_path
                .parent()
                .expect("alert checkpoint parent"),
        )
        .expect("alert checkpoint parent should be created");
    }
    if let Some(journal_gc) = &config.journal_gc {
        fs::create_dir_all(&journal_gc.journal_root).expect("journal root should be created");
        if let Some(artifact_root) = &journal_gc.artifact_root {
            fs::create_dir_all(artifact_root).expect("artifact root should be created");
        }
    }
    if let Some(drift_audit) = &config.drift_audit {
        fs::create_dir_all(&drift_audit.expected_shadow_root)
            .expect("expected shadow root should be created");
        fs::create_dir_all(&drift_audit.observed_shadow_root)
            .expect("observed shadow root should be created");
    }
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-worker-deployment-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
