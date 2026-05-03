use std::future::Future;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::state::{
    missing_shadow_state, DeviceShadowState, JsonFileShadowStateStore, ShadowStateStore,
};
use crate::telemetry::{
    EventSink, JsonFileOperationAlertCheckpointStore, JsonFileOperationAlertSink,
    JsonFileOperationSummaryStore, NoopEventSink, OperationSummaryRetentionPolicy,
    RecordingEventSink,
};
use crate::worker::drift_auditor::{
    DriftAuditSchedule, DriftAuditWorker, DriftAuditor, DriftObservationSource,
};
use crate::worker::gc::{JournalGc, JournalGcSchedule, JournalGcWorker, RetentionPolicy};
use crate::worker::operation_alerts::{
    OperationAlertDeliverySchedule, OperationAlertDeliveryWorker,
};
use crate::worker::operation_summary_compactor::{
    OperationSummaryCompactionSchedule, OperationSummaryCompactionWorker,
};
use crate::worker::runtime::{UnderlayWorkerRuntime, UnderlayWorkerRuntimeReport};
use crate::utils::atomic_file::atomic_write;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnderlayWorkerDaemonConfig {
    #[serde(default)]
    pub operation_summary: Option<OperationSummaryDaemonConfig>,
    #[serde(default)]
    pub operation_alert: Option<OperationAlertDaemonConfig>,
    #[serde(default)]
    pub journal_gc: Option<JournalGcDaemonConfig>,
    #[serde(default)]
    pub drift_audit: Option<DriftAuditDaemonConfig>,
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

#[derive(Debug)]
struct ShadowStoreObservationSource {
    observed_store: Arc<dyn ShadowStateStore>,
}

impl UnderlayWorkerDaemonConfig {
    pub fn from_path(path: impl AsRef<Path>) -> UnderlayResult<Self> {
        let path = path.as_ref();
        let payload = fs::read(path).map_err(worker_config_io_error)?;
        serde_json::from_slice(&payload).map_err(|err| {
            UnderlayError::InvalidIntent(format!("parse worker daemon config {:?}: {err}", path))
        })
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
        let event_sink: Arc<dyn EventSink> = match operation_summary_store.clone() {
            Some(store) => Arc::new(RecordingEventSink::new(Arc::new(NoopEventSink), store)),
            None => Arc::new(NoopEventSink),
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

fn worker_config_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("worker daemon config io error: {err}"))
}
