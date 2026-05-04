use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::worker::daemon::{
    DriftAuditDaemonConfig, JournalGcDaemonConfig, OperationAlertDaemonConfig,
    OperationAuditDaemonConfig, OperationSummaryDaemonConfig, UnderlayWorkerDaemonConfig,
    WorkerReloadDaemonConfig, WorkerScheduleConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDeploymentPreflightReport {
    pub valid: bool,
    pub strict_paths: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub checked_paths: Vec<WorkerDeploymentPathCheck>,
}

impl WorkerDeploymentPreflightReport {
    fn new(strict_paths: bool) -> Self {
        Self {
            valid: true,
            strict_paths,
            errors: Vec::new(),
            warnings: Vec::new(),
            checked_paths: Vec::new(),
        }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.valid = false;
        self.errors.push(message.into());
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDeploymentPathCheck {
    pub path: PathBuf,
    pub kind: String,
    pub required: bool,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct WorkerDeploymentPreflight {
    strict_paths: bool,
}

impl WorkerDeploymentPreflight {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn strict_paths(mut self, strict_paths: bool) -> Self {
        self.strict_paths = strict_paths;
        self
    }

    pub fn check_config_path(
        &self,
        path: impl AsRef<Path>,
    ) -> WorkerDeploymentPreflightReport {
        let path = path.as_ref();
        let mut report = WorkerDeploymentPreflightReport::new(self.strict_paths);
        self.check_file_exists(&mut report, path, "worker_config.path", true);

        match UnderlayWorkerDaemonConfig::from_path(path) {
            Ok(config) => self.check_config_into(&mut report, &config),
            Err(err) => report.error(format!("worker_config: {err}")),
        }
        report
    }

    pub fn check_config(
        &self,
        config: &UnderlayWorkerDaemonConfig,
    ) -> WorkerDeploymentPreflightReport {
        let mut report = WorkerDeploymentPreflightReport::new(self.strict_paths);
        self.check_config_into(&mut report, config);
        report
    }

    fn check_config_into(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &UnderlayWorkerDaemonConfig,
    ) {
        if config.operation_summary.is_none()
            && config.operation_audit.is_none()
            && config.operation_alert.is_none()
            && config.journal_gc.is_none()
            && config.drift_audit.is_none()
        {
            report.warning("worker config has no enabled sections");
        }

        if config.operation_alert.is_some() && config.operation_summary.is_none() {
            report.error("operation_alert requires operation_summary.path");
        }

        if let Some(operation_summary) = &config.operation_summary {
            self.check_operation_summary(report, operation_summary);
        }
        if let Some(operation_audit) = &config.operation_audit {
            self.check_operation_audit(report, operation_audit);
        }
        if let Some(operation_alert) = &config.operation_alert {
            self.check_operation_alert(report, operation_alert);
        }
        if let Some(journal_gc) = &config.journal_gc {
            self.check_journal_gc(report, journal_gc);
        }
        if let Some(drift_audit) = &config.drift_audit {
            self.check_drift_audit(report, drift_audit);
        }
        if let Some(reload) = &config.reload {
            self.check_reload(report, reload);
        }
    }

    fn check_operation_summary(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &OperationSummaryDaemonConfig,
    ) {
        if let Err(err) = config.retention.validate() {
            report.error(format!("operation_summary.retention: {err}"));
        }
        check_schedule(
            report,
            "operation_summary.retention_schedule",
            config.retention_schedule,
        );
        self.check_file_parent(
            report,
            &config.path,
            "operation_summary.path.parent",
            true,
        );
    }

    fn check_operation_audit(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &OperationAuditDaemonConfig,
    ) {
        if let Err(err) = config.retention.validate() {
            report.error(format!("operation_audit.retention: {err}"));
        }
        check_schedule(
            report,
            "operation_audit.retention_schedule",
            config.retention_schedule,
        );
        self.check_file_parent(report, &config.path, "operation_audit.path.parent", true);
    }

    fn check_operation_alert(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &OperationAlertDaemonConfig,
    ) {
        check_schedule(report, "operation_alert.schedule", config.schedule);
        self.check_file_parent(report, &config.path, "operation_alert.path.parent", true);
        self.check_file_parent(
            report,
            &config.checkpoint_path,
            "operation_alert.checkpoint_path.parent",
            true,
        );
    }

    fn check_journal_gc(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &JournalGcDaemonConfig,
    ) {
        if let Err(err) = config.retention.validate() {
            report.error(format!("journal_gc.retention: {err}"));
        }
        check_schedule(report, "journal_gc.schedule", config.schedule);
        self.check_directory(report, &config.journal_root, "journal_gc.journal_root", true);
        if let Some(artifact_root) = &config.artifact_root {
            self.check_directory(report, artifact_root, "journal_gc.artifact_root", true);
        }
    }

    fn check_drift_audit(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &DriftAuditDaemonConfig,
    ) {
        check_schedule(report, "drift_audit.schedule", config.schedule);
        self.check_directory(
            report,
            &config.expected_shadow_root,
            "drift_audit.expected_shadow_root",
            true,
        );
        self.check_directory(
            report,
            &config.observed_shadow_root,
            "drift_audit.observed_shadow_root",
            true,
        );
    }

    fn check_reload(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        config: &WorkerReloadDaemonConfig,
    ) {
        if !config.enabled {
            return;
        }
        if config.poll_interval_secs == 0 {
            report.error("reload.poll_interval_secs must be greater than zero");
        }
        let Some(checkpoint_path) = &config.checkpoint_path else {
            report.error("reload.checkpoint_path is required when reload is enabled");
            return;
        };
        self.check_file_parent(
            report,
            checkpoint_path,
            "reload.checkpoint_path.parent",
            true,
        );
    }

    fn check_file_parent(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        path: &Path,
        kind: &str,
        required: bool,
    ) {
        let Some(parent) = path.parent() else {
            report.error(format!("{kind}: path {:?} has no parent directory", path));
            return;
        };
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        self.check_directory(report, parent, kind, required);
    }

    fn check_file_exists(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        path: &Path,
        kind: &str,
        required: bool,
    ) {
        let exists = path.is_file();
        report.checked_paths.push(WorkerDeploymentPathCheck {
            path: path.to_path_buf(),
            kind: kind.into(),
            required,
            exists,
            writable: None,
        });
        if required && !exists {
            report.error(format!("{kind}: missing required file {:?}", path));
        }
    }

    fn check_directory(
        &self,
        report: &mut WorkerDeploymentPreflightReport,
        path: &Path,
        kind: &str,
        required: bool,
    ) {
        let exists = path.is_dir();
        let writable = if self.strict_paths && exists {
            Some(can_write_to_directory(path))
        } else {
            None
        };

        report.checked_paths.push(WorkerDeploymentPathCheck {
            path: path.to_path_buf(),
            kind: kind.into(),
            required,
            exists,
            writable,
        });

        if self.strict_paths && required && !exists {
            report.error(format!("{kind}: missing required directory {:?}", path));
        }
        if self.strict_paths && writable == Some(false) {
            report.error(format!("{kind}: directory is not writable {:?}", path));
        }
    }
}

fn check_schedule(
    report: &mut WorkerDeploymentPreflightReport,
    field: &str,
    schedule: WorkerScheduleConfig,
) {
    if schedule.interval_secs == 0 {
        report.error(format!(
            "{field}.interval_secs must be greater than zero"
        ));
    }
}

fn can_write_to_directory(path: &Path) -> bool {
    let marker = path.join(format!(
        ".aria-underlay-preflight-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
    {
        Ok(_) => {
            let _ = fs::remove_file(marker);
            true
        }
        Err(_) => false,
    }
}
