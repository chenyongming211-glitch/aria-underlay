use std::future::Future;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::state::{
    missing_shadow_state, DeviceShadowState, JsonFileShadowStateStore, ShadowStateStore,
};
use crate::telemetry::{
    EventSink, JsonFileOperationAlertCheckpointStore, JsonFileOperationAlertSink,
    JsonFileOperationAuditStore, JsonFileOperationSummaryStore,
    OperationAuditRetentionPolicy, OperationSummaryRetentionPolicy, RecordingEventSink,
    StderrEventSink,
};
use crate::worker::drift_auditor::{
    DriftAuditSchedule, DriftAuditWorker, DriftAuditor, DriftObservationSource,
};
use crate::worker::gc::{JournalGc, JournalGcSchedule, JournalGcWorker, RetentionPolicy};
use crate::worker::operation_alerts::{
    OperationAlertDeliverySchedule, OperationAlertDeliveryWorker,
};
use crate::worker::operation_audit_compactor::{
    OperationAuditCompactionSchedule, OperationAuditCompactionWorker,
};
use crate::worker::operation_summary_compactor::{
    OperationSummaryCompactionSchedule, OperationSummaryCompactionWorker,
};
use crate::worker::runtime::{UnderlayWorkerRuntime, UnderlayWorkerRuntimeReport};
use crate::utils::atomic_file::atomic_write;
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnderlayWorkerDaemonConfig {
    #[serde(default)]
    pub reload: Option<WorkerReloadDaemonConfig>,
    #[serde(default)]
    pub operation_summary: Option<OperationSummaryDaemonConfig>,
    #[serde(default)]
    pub operation_audit: Option<OperationAuditDaemonConfig>,
    #[serde(default)]
    pub operation_alert: Option<OperationAlertDaemonConfig>,
    #[serde(default)]
    pub journal_gc: Option<JournalGcDaemonConfig>,
    #[serde(default)]
    pub drift_audit: Option<DriftAuditDaemonConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerReloadDaemonConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_reload_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub checkpoint_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationSummaryDaemonConfig {
    pub path: PathBuf,
    #[serde(default)]
    pub retention: OperationSummaryRetentionPolicy,
    #[serde(default)]
    pub retention_schedule: WorkerScheduleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationAuditDaemonConfig {
    pub path: PathBuf,
    #[serde(default)]
    pub retention: OperationAuditRetentionPolicy,
    #[serde(default)]
    pub retention_schedule: WorkerScheduleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationAlertDaemonConfig {
    pub path: PathBuf,
    pub checkpoint_path: PathBuf,
    #[serde(default)]
    pub schedule: WorkerScheduleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalGcDaemonConfig {
    pub journal_root: PathBuf,
    #[serde(default)]
    pub artifact_root: Option<PathBuf>,
    #[serde(default)]
    pub schedule: WorkerScheduleConfig,
    #[serde(default)]
    pub retention: RetentionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAuditDaemonConfig {
    pub expected_shadow_root: PathBuf,
    pub observed_shadow_root: PathBuf,
    #[serde(default)]
    pub schedule: WorkerScheduleConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerScheduleConfig {
    pub interval_secs: u64,
    #[serde(default = "default_run_immediately")]
    pub run_immediately: bool,
}

impl Default for WorkerScheduleConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60 * 60,
            run_immediately: true,
        }
    }
}

#[derive(Debug)]
pub struct UnderlayWorkerDaemon {
    runtime: UnderlayWorkerRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerConfigReloadStatus {
    Started,
    Applied,
    Rejected,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerReloadCheckpoint {
    pub config_path: PathBuf,
    pub generation: u64,
    pub fingerprint: String,
    pub status: WorkerConfigReloadStatus,
    pub updated_at_unix_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl WorkerReloadCheckpoint {
    pub fn from_path(path: impl AsRef<Path>) -> UnderlayResult<Self> {
        let path = path.as_ref();
        let payload = fs::read(path).map_err(|err| {
            UnderlayError::InvalidIntent(format!(
                "read worker reload checkpoint {:?}: {err}",
                path
            ))
        })?;
        serde_json::from_slice(&payload).map_err(|err| {
            UnderlayError::InvalidIntent(format!(
                "parse worker reload checkpoint {:?}: {err}",
                path
            ))
        })
    }
}

#[derive(Debug)]
struct ShadowStoreObservationSource {
    observed_store: Arc<dyn ShadowStateStore>,
}

struct RunningWorkerRuntime {
    shutdown_tx: watch::Sender<bool>,
    task: JoinHandle<UnderlayResult<UnderlayWorkerRuntimeReport>>,
}

impl UnderlayWorkerDaemonConfig {
    pub fn from_path(path: impl AsRef<Path>) -> UnderlayResult<Self> {
        let path = path.as_ref();
        let payload = fs::read(path).map_err(worker_config_io_error)?;
        parse_worker_config_payload(path, &payload)
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> UnderlayResult<()> {
        let payload = serde_json::to_vec_pretty(self).map_err(|err| {
            UnderlayError::Internal(format!("serialize worker daemon config: {err}"))
        })?;
        atomic_write(path.as_ref(), &payload, worker_config_io_error)
    }
}

impl UnderlayWorkerDaemon {
    pub fn from_config(config: UnderlayWorkerDaemonConfig) -> UnderlayResult<Self> {
        let operation_summary_config = config.operation_summary;
        let operation_summary_store = operation_summary_config
            .as_ref()
            .map(|config| Arc::new(JsonFileOperationSummaryStore::new(config.path.clone())));
        let operation_audit_config = config.operation_audit;
        let operation_audit_store = operation_audit_config
            .as_ref()
            .map(|config| Arc::new(JsonFileOperationAuditStore::new(config.path.clone())));
        let runtime_log_sink: Arc<dyn EventSink> = Arc::new(StderrEventSink);
        let event_sink: Arc<dyn EventSink> =
            match (operation_summary_store.clone(), operation_audit_store.clone()) {
                (Some(summary_store), Some(audit_store)) => Arc::new(
                    RecordingEventSink::new(runtime_log_sink.clone(), summary_store)
                        .with_operation_audit_store(audit_store),
                ),
                (Some(summary_store), None) => {
                    Arc::new(RecordingEventSink::new(runtime_log_sink.clone(), summary_store))
                }
                (None, Some(audit_store)) => Arc::new(
                    RecordingEventSink::new(
                        runtime_log_sink.clone(),
                        Arc::new(crate::telemetry::InMemoryOperationSummaryStore::default()),
                    )
                    .with_operation_audit_store(audit_store),
                ),
                (None, None) => runtime_log_sink,
            };

        let mut runtime = UnderlayWorkerRuntime::new();
        if let (Some(config), Some(store)) =
            (operation_summary_config, operation_summary_store.clone())
        {
            if config.retention.is_enabled() {
                runtime = runtime.with_operation_summary_compaction(
                    OperationSummaryCompactionWorker::new(store, config.retention),
                    config.retention_schedule.into(),
                );
            }
        }
        if let (Some(config), Some(store)) =
            (operation_audit_config, operation_audit_store.clone())
        {
            if config.retention.is_enabled() {
                runtime = runtime.with_operation_audit_compaction(
                    OperationAuditCompactionWorker::new(store, config.retention),
                    config.retention_schedule.into(),
                );
            }
        }
        if let Some(config) = config.operation_alert {
            let Some(store) = operation_summary_store.clone() else {
                return Err(UnderlayError::InvalidIntent(
                    "operation alert delivery requires operation_summary.path".into(),
                ));
            };
            runtime = runtime.with_operation_alert_delivery(
                OperationAlertDeliveryWorker::new(
                    store,
                    Arc::new(JsonFileOperationAlertSink::new(config.path)),
                    Arc::new(JsonFileOperationAlertCheckpointStore::new(
                        config.checkpoint_path,
                    )),
                ),
                config.schedule.into(),
            );
        }
        if let Some(config) = config.journal_gc {
            let mut gc = JournalGc::new(config.journal_root);
            if let Some(artifact_root) = config.artifact_root {
                gc = gc.with_artifact_root(artifact_root);
            }
            runtime = runtime.with_journal_gc(
                JournalGcWorker::new(gc, config.retention, event_sink.clone()),
                config.schedule.into(),
            );
        }

        if let Some(config) = config.drift_audit {
            let expected_store = Arc::new(JsonFileShadowStateStore::new(config.expected_shadow_root));
            let observed_source = Arc::new(ShadowStoreObservationSource {
                observed_store: Arc::new(JsonFileShadowStateStore::new(config.observed_shadow_root)),
            });
            runtime = runtime.with_drift_audit(
                DriftAuditWorker::new(
                    DriftAuditor::from_source(expected_store, observed_source),
                    event_sink,
                ),
                config.schedule.into(),
            );
        }

        Ok(Self { runtime })
    }

    pub async fn run_config_path_until_shutdown<F>(
        path: impl AsRef<Path>,
        shutdown: F,
    ) -> UnderlayResult<UnderlayWorkerRuntimeReport>
    where
        F: Future<Output = ()>,
    {
        let config_path = path.as_ref().to_path_buf();
        let initial_payload = fs::read(&config_path).map_err(worker_config_io_error)?;
        let initial_config = parse_worker_config_payload(&config_path, &initial_payload)?;
        validate_worker_config_for_runtime(&initial_config)?;

        let Some(reload_config) = enabled_reload_config(&initial_config) else {
            return Self::from_config(initial_config)?
                .run_until_shutdown(shutdown)
                .await;
        };

        run_reloadable_config_path_until_shutdown(
            config_path,
            initial_payload,
            initial_config,
            reload_config,
            shutdown,
        )
        .await
    }

    pub async fn run_until_shutdown<F>(
        self,
        shutdown: F,
    ) -> UnderlayResult<UnderlayWorkerRuntimeReport>
    where
        F: Future<Output = ()>,
    {
        self.runtime.run_until_shutdown(shutdown).await
    }
}

#[async_trait]
impl DriftObservationSource for ShadowStoreObservationSource {
    async fn get_observed_state(
        &self,
        device_id: &crate::model::DeviceId,
    ) -> UnderlayResult<DeviceShadowState> {
        self.observed_store
            .get(device_id)?
            .ok_or_else(|| missing_shadow_state(device_id))
    }
}

impl From<WorkerScheduleConfig> for JournalGcSchedule {
    fn from(config: WorkerScheduleConfig) -> Self {
        Self {
            interval_secs: config.interval_secs,
            run_immediately: config.run_immediately,
        }
    }
}

impl From<WorkerScheduleConfig> for DriftAuditSchedule {
    fn from(config: WorkerScheduleConfig) -> Self {
        Self {
            interval_secs: config.interval_secs,
            run_immediately: config.run_immediately,
        }
    }
}

impl From<WorkerScheduleConfig> for OperationSummaryCompactionSchedule {
    fn from(config: WorkerScheduleConfig) -> Self {
        Self {
            interval_secs: config.interval_secs,
            run_immediately: config.run_immediately,
        }
    }
}

impl From<WorkerScheduleConfig> for OperationAuditCompactionSchedule {
    fn from(config: WorkerScheduleConfig) -> Self {
        Self {
            interval_secs: config.interval_secs,
            run_immediately: config.run_immediately,
        }
    }
}

impl From<WorkerScheduleConfig> for OperationAlertDeliverySchedule {
    fn from(config: WorkerScheduleConfig) -> Self {
        Self {
            interval_secs: config.interval_secs,
            run_immediately: config.run_immediately,
        }
    }
}

fn default_run_immediately() -> bool {
    true
}

fn default_reload_poll_interval_secs() -> u64 {
    5
}

async fn run_reloadable_config_path_until_shutdown<F>(
    config_path: PathBuf,
    initial_payload: Vec<u8>,
    initial_config: UnderlayWorkerDaemonConfig,
    reload_config: WorkerReloadDaemonConfig,
    shutdown: F,
) -> UnderlayResult<UnderlayWorkerRuntimeReport>
where
    F: Future<Output = ()>,
{
    let checkpoint_path = reload_config
        .checkpoint_path
        .clone()
        .ok_or_else(|| invalid_reload_config("reload.checkpoint_path is required when enabled"))?;
    let mut generation = 1;
    let mut adopted_fingerprint = payload_fingerprint(&initial_payload);
    let mut last_seen_fingerprint = adopted_fingerprint.clone();
    write_reload_checkpoint(
        &checkpoint_path,
        reload_checkpoint(
            &config_path,
            generation,
            &adopted_fingerprint,
            WorkerConfigReloadStatus::Started,
            None,
        ),
    )?;

    let mut running = RunningWorkerRuntime::spawn(UnderlayWorkerDaemon::from_config(
        initial_config,
    )?);
    let mut poll = tokio::time::interval(Duration::from_secs(reload_config.poll_interval_secs));
    poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
    poll.tick().await;

    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                let report = running.stop().await?;
                write_reload_checkpoint(
                    &checkpoint_path,
                    reload_checkpoint(
                        &config_path,
                        generation,
                        &adopted_fingerprint,
                        WorkerConfigReloadStatus::Shutdown,
                        None,
                    ),
                )?;
                return Ok(report);
            }
            _ = poll.tick() => {
                let payload = match fs::read(&config_path) {
                    Ok(payload) => payload,
                    Err(err) => {
                        let message = format!("read worker daemon config {:?}: {err}", config_path);
                        write_reload_checkpoint(
                            &checkpoint_path,
                            reload_checkpoint(
                                &config_path,
                                generation,
                                &adopted_fingerprint,
                                WorkerConfigReloadStatus::Rejected,
                                Some(message),
                            ),
                        )?;
                        continue;
                    }
                };
                let candidate_fingerprint = payload_fingerprint(&payload);
                if candidate_fingerprint == last_seen_fingerprint {
                    continue;
                }
                last_seen_fingerprint = candidate_fingerprint.clone();

                let candidate = parse_worker_config_payload(&config_path, &payload)
                    .and_then(|config| {
                        validate_worker_config_for_runtime(&config)?;
                        Ok(config)
                    });
                let candidate = match candidate {
                    Ok(candidate) => candidate,
                    Err(err) => {
                        write_reload_checkpoint(
                            &checkpoint_path,
                            reload_checkpoint(
                                &config_path,
                                generation,
                                &adopted_fingerprint,
                                WorkerConfigReloadStatus::Rejected,
                                Some(err.to_string()),
                            ),
                        )?;
                        continue;
                    }
                };

                let candidate_daemon = match UnderlayWorkerDaemon::from_config(candidate) {
                    Ok(daemon) => daemon,
                    Err(err) => {
                        write_reload_checkpoint(
                            &checkpoint_path,
                            reload_checkpoint(
                                &config_path,
                                generation,
                                &adopted_fingerprint,
                                WorkerConfigReloadStatus::Rejected,
                                Some(err.to_string()),
                            ),
                        )?;
                        continue;
                    }
                };

                let _previous_report = running.stop().await?;
                running = RunningWorkerRuntime::spawn(candidate_daemon);
                generation += 1;
                adopted_fingerprint = candidate_fingerprint;
                write_reload_checkpoint(
                    &checkpoint_path,
                    reload_checkpoint(
                        &config_path,
                        generation,
                        &adopted_fingerprint,
                        WorkerConfigReloadStatus::Applied,
                        None,
                    ),
                )?;
            }
        }
    }
}

impl RunningWorkerRuntime {
    fn spawn(daemon: UnderlayWorkerDaemon) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            daemon
                .run_until_shutdown(wait_for_daemon_runtime_shutdown(shutdown_rx))
                .await
        });
        Self { shutdown_tx, task }
    }

    async fn stop(self) -> UnderlayResult<UnderlayWorkerRuntimeReport> {
        let _ = self.shutdown_tx.send(true);
        self.task.await.map_err(worker_runtime_join_error)?
    }
}

async fn wait_for_daemon_runtime_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

fn parse_worker_config_payload(
    path: &Path,
    payload: &[u8],
) -> UnderlayResult<UnderlayWorkerDaemonConfig> {
    serde_json::from_slice(payload).map_err(|err| {
        UnderlayError::InvalidIntent(format!("parse worker daemon config {:?}: {err}", path))
    })
}

fn enabled_reload_config(config: &UnderlayWorkerDaemonConfig) -> Option<WorkerReloadDaemonConfig> {
    config
        .reload
        .as_ref()
        .filter(|reload| reload.enabled)
        .cloned()
}

pub(crate) fn validate_worker_config_for_runtime(
    config: &UnderlayWorkerDaemonConfig,
) -> UnderlayResult<()> {
    if config.operation_alert.is_some() && config.operation_summary.is_none() {
        return Err(UnderlayError::InvalidIntent(
            "operation_alert requires operation_summary.path".into(),
        ));
    }
    if let Some(operation_summary) = &config.operation_summary {
        operation_summary.retention.validate()?;
        validate_worker_schedule(
            "operation_summary.retention_schedule",
            operation_summary.retention_schedule,
        )?;
    }
    if let Some(operation_audit) = &config.operation_audit {
        operation_audit.retention.validate()?;
        validate_worker_schedule(
            "operation_audit.retention_schedule",
            operation_audit.retention_schedule,
        )?;
    }
    if let Some(operation_alert) = &config.operation_alert {
        validate_worker_schedule("operation_alert.schedule", operation_alert.schedule)?;
    }
    if let Some(journal_gc) = &config.journal_gc {
        journal_gc.retention.validate()?;
        validate_worker_schedule("journal_gc.schedule", journal_gc.schedule)?;
    }
    if let Some(drift_audit) = &config.drift_audit {
        validate_worker_schedule("drift_audit.schedule", drift_audit.schedule)?;
    }
    if let Some(reload) = &config.reload {
        validate_reload_config(reload)?;
    }
    Ok(())
}

fn validate_reload_config(config: &WorkerReloadDaemonConfig) -> UnderlayResult<()> {
    if !config.enabled {
        return Ok(());
    }
    if config.poll_interval_secs == 0 {
        return Err(invalid_reload_config(
            "reload.poll_interval_secs must be greater than zero",
        ));
    }
    if config.checkpoint_path.is_none() {
        return Err(invalid_reload_config(
            "reload.checkpoint_path is required when enabled",
        ));
    }
    Ok(())
}

fn validate_worker_schedule(field: &str, schedule: WorkerScheduleConfig) -> UnderlayResult<()> {
    if schedule.interval_secs == 0 {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field}.interval_secs must be greater than zero"
        )));
    }
    Ok(())
}

fn invalid_reload_config(message: impl Into<String>) -> UnderlayError {
    UnderlayError::InvalidIntent(message.into())
}

fn reload_checkpoint(
    config_path: &Path,
    generation: u64,
    fingerprint: &str,
    status: WorkerConfigReloadStatus,
    error: Option<String>,
) -> WorkerReloadCheckpoint {
    WorkerReloadCheckpoint {
        config_path: config_path.to_path_buf(),
        generation,
        fingerprint: fingerprint.into(),
        status,
        updated_at_unix_secs: now_unix_secs(),
        error,
    }
}

fn write_reload_checkpoint(path: &Path, checkpoint: WorkerReloadCheckpoint) -> UnderlayResult<()> {
    let payload = serde_json::to_vec_pretty(&checkpoint).map_err(|err| {
        UnderlayError::Internal(format!("serialize worker reload checkpoint: {err}"))
    })?;
    atomic_write(path, &payload, worker_config_io_error)
}

fn payload_fingerprint(payload: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in payload {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}:{}", hash, payload.len())
}

fn worker_runtime_join_error(err: tokio::task::JoinError) -> UnderlayError {
    UnderlayError::Internal(format!("worker daemon runtime task join error: {err}"))
}

fn worker_config_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("worker daemon config io error: {err}"))
}
