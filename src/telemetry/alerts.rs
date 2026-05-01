use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::ops::OperationSummary;
use crate::utils::atomic_file::atomic_write;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationAlertSeverity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAlert {
    pub dedupe_key: String,
    pub severity: OperationAlertSeverity,
    pub request_id: String,
    pub trace_id: String,
    pub action: String,
    pub result: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub fields: BTreeMap<String, String>,
}

pub trait OperationAlertSink: std::fmt::Debug + Send + Sync {
    fn deliver(&self, alerts: &[OperationAlert]) -> UnderlayResult<()>;
}

pub trait OperationAlertCheckpointStore: std::fmt::Debug + Send + Sync {
    fn delivered_keys(&self) -> UnderlayResult<BTreeSet<String>>;
    fn record_delivered(&self, keys: &[String]) -> UnderlayResult<()>;
}

impl OperationAlert {
    pub fn from_summary(summary: OperationSummary) -> Self {
        Self {
            dedupe_key: dedupe_key(&summary),
            severity: severity_for_summary(&summary),
            request_id: summary.request_id,
            trace_id: summary.trace_id,
            action: summary.action,
            result: summary.result,
            tx_id: summary.tx_id,
            device_id: summary.device_id,
            fields: summary.fields,
        }
    }
}

#[derive(Debug, Default)]
pub struct InMemoryOperationAlertSink {
    alerts: Mutex<Vec<OperationAlert>>,
}

impl InMemoryOperationAlertSink {
    pub fn alerts(&self) -> Vec<OperationAlert> {
        self.alerts
            .lock()
            .expect("operation alert sink mutex should not be poisoned")
            .clone()
    }
}

impl OperationAlertSink for InMemoryOperationAlertSink {
    fn deliver(&self, alerts: &[OperationAlert]) -> UnderlayResult<()> {
        self.alerts
            .lock()
            .map_err(|_| UnderlayError::Internal("operation alert sink mutex poisoned".into()))?
            .extend(alerts.iter().cloned());
        Ok(())
    }
}

#[derive(Debug)]
pub struct JsonFileOperationAlertSink {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileOperationAlertSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> UnderlayResult<Vec<OperationAlert>> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("operation alert file mutex poisoned".into()))?;
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let payload = fs::read_to_string(&self.path).map_err(alert_io_error)?;
        parse_alert_lines(&self.path, &payload)
    }
}

impl OperationAlertSink for JsonFileOperationAlertSink {
    fn deliver(&self, alerts: &[OperationAlert]) -> UnderlayResult<()> {
        if alerts.is_empty() {
            return Ok(());
        }

        let _guard = self
            .lock
            .lock()
            .map_err(|_| UnderlayError::Internal("operation alert file mutex poisoned".into()))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(alert_io_error)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(alert_io_error)?;
        for alert in alerts {
            let mut payload = serde_json::to_vec(alert)
                .map_err(|err| UnderlayError::Internal(format!("serialize operation alert: {err}")))?;
            payload.push(b'\n');
            file.write_all(&payload).map_err(alert_io_error)?;
        }
        file.sync_all().map_err(alert_io_error)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryOperationAlertCheckpointStore {
    delivered_keys: Mutex<BTreeSet<String>>,
}

impl InMemoryOperationAlertCheckpointStore {
    pub fn delivered_keys(&self) -> UnderlayResult<BTreeSet<String>> {
        <Self as OperationAlertCheckpointStore>::delivered_keys(self)
    }
}

impl OperationAlertCheckpointStore for InMemoryOperationAlertCheckpointStore {
    fn delivered_keys(&self) -> UnderlayResult<BTreeSet<String>> {
        Ok(self
            .delivered_keys
            .lock()
            .map_err(|_| {
                UnderlayError::Internal("operation alert checkpoint mutex poisoned".into())
            })?
            .clone())
    }

    fn record_delivered(&self, keys: &[String]) -> UnderlayResult<()> {
        let mut delivered_keys = self.delivered_keys.lock().map_err(|_| {
            UnderlayError::Internal("operation alert checkpoint mutex poisoned".into())
        })?;
        delivered_keys.extend(keys.iter().cloned());
        Ok(())
    }
}

#[derive(Debug)]
pub struct JsonFileOperationAlertCheckpointStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileOperationAlertCheckpointStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }
}

impl OperationAlertCheckpointStore for JsonFileOperationAlertCheckpointStore {
    fn delivered_keys(&self) -> UnderlayResult<BTreeSet<String>> {
        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("operation alert checkpoint mutex poisoned".into())
        })?;
        read_checkpoint_keys(&self.path)
    }

    fn record_delivered(&self, keys: &[String]) -> UnderlayResult<()> {
        if keys.is_empty() {
            return Ok(());
        }

        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("operation alert checkpoint mutex poisoned".into())
        })?;
        let mut delivered_keys = read_checkpoint_keys(&self.path)?;
        delivered_keys.extend(keys.iter().cloned());
        let checkpoint = OperationAlertCheckpoint { delivered_keys };
        let payload = serde_json::to_vec_pretty(&checkpoint).map_err(|err| {
            UnderlayError::Internal(format!("serialize operation alert checkpoint: {err}"))
        })?;
        atomic_write(&self.path, &payload, alert_io_error)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OperationAlertCheckpoint {
    delivered_keys: BTreeSet<String>,
}

fn read_checkpoint_keys(path: &Path) -> UnderlayResult<BTreeSet<String>> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }

    let payload = fs::read_to_string(path).map_err(alert_io_error)?;
    let checkpoint = serde_json::from_str::<OperationAlertCheckpoint>(&payload).map_err(|err| {
        UnderlayError::Internal(format!("parse operation alert checkpoint {:?}: {err}", path))
    })?;
    Ok(checkpoint.delivered_keys)
}

fn parse_alert_lines(path: &Path, payload: &str) -> UnderlayResult<Vec<OperationAlert>> {
    let mut alerts = Vec::new();
    for (index, line) in payload.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let alert = serde_json::from_str::<OperationAlert>(line).map_err(|err| {
            UnderlayError::Internal(format!(
                "parse operation alert {:?} line {}: {err}",
                path,
                index + 1
            ))
        })?;
        alerts.push(alert);
    }
    Ok(alerts)
}

fn dedupe_key(summary: &OperationSummary) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        summary.action,
        summary.result,
        summary.request_id,
        summary.trace_id,
        summary.tx_id.as_deref().unwrap_or("-"),
        summary
            .device_id
            .as_ref()
            .map(|device_id| device_id.0.as_str())
            .unwrap_or("-")
    )
}

fn severity_for_summary(summary: &OperationSummary) -> OperationAlertSeverity {
    if matches!(summary.action.as_str(), "transaction.in_doubt" | "audit.write_failed")
        || matches!(summary.result.as_str(), "in_doubt" | "failed")
    {
        OperationAlertSeverity::Critical
    } else {
        OperationAlertSeverity::Warning
    }
}

fn alert_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("operation alert io error: {err}"))
}
