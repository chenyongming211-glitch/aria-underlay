use std::sync::Arc;
use std::{future::Future, time::Duration};

use async_trait::async_trait;
use tokio::time::MissedTickBehavior;

use crate::model::DeviceId;
use crate::state::drift::{detect_drift, DriftReport};
use crate::state::{DeviceShadowState, ShadowStateStore};
use crate::telemetry::{EventSink, UnderlayEvent};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Default, Clone)]
pub struct DriftAuditor {
    snapshots: Vec<DriftAuditSnapshot>,
    expected_store: Option<Arc<dyn ShadowStateStore>>,
    observed_source: Option<Arc<dyn DriftObservationSource>>,
}

#[derive(Debug, Clone)]
pub struct DriftAuditSnapshot {
    pub expected: DeviceShadowState,
    pub observed: DeviceShadowState,
}

#[async_trait]
pub trait DriftObservationSource: std::fmt::Debug + Send + Sync {
    async fn get_observed_state(&self, device_id: &DeviceId) -> UnderlayResult<DeviceShadowState>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriftAuditRunSummary {
    pub audited_devices: usize,
    pub failed_devices: Vec<DeviceId>,
    pub drifted_devices: Vec<DeviceId>,
    pub reports: Vec<DriftReport>,
}

impl DriftAuditRunSummary {
    fn from_reports(audited_devices: usize, reports: Vec<DriftReport>) -> Self {
        Self::from_reports_and_failures(audited_devices, reports, Vec::new())
    }

    fn from_reports_and_failures(
        audited_devices: usize,
        reports: Vec<DriftReport>,
        failed_devices: Vec<DeviceId>,
    ) -> Self {
        let drifted_devices = reports
            .iter()
            .map(|report| report.device_id.clone())
            .collect::<Vec<_>>();
        Self {
            audited_devices,
            failed_devices,
            drifted_devices,
            reports,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftAuditSchedule {
    pub interval_secs: u64,
    pub run_immediately: bool,
}

impl Default for DriftAuditSchedule {
    fn default() -> Self {
        Self {
            interval_secs: 5 * 60,
            run_immediately: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriftAuditSchedulerReport {
    pub runs: usize,
    pub last_summary: Option<DriftAuditRunSummary>,
}

#[derive(Debug)]
pub struct DriftAuditWorker {
    auditor: DriftAuditor,
    event_sink: Arc<dyn EventSink>,
    request_id: String,
    trace_id: String,
}

impl DriftAuditor {
    pub fn new(snapshots: Vec<DriftAuditSnapshot>) -> Self {
        Self {
            snapshots,
            expected_store: None,
            observed_source: None,
        }
    }

    pub fn from_source(
        expected_store: Arc<dyn ShadowStateStore>,
        observed_source: Arc<dyn DriftObservationSource>,
    ) -> Self {
        Self {
            snapshots: Vec::new(),
            expected_store: Some(expected_store),
            observed_source: Some(observed_source),
        }
    }

    pub async fn run_once(&self) -> UnderlayResult<Vec<DriftReport>> {
        Ok(self.run_once_with_summary().await?.reports)
    }

    pub async fn run_once_with_summary(&self) -> UnderlayResult<DriftAuditRunSummary> {
        if let (Some(expected_store), Some(observed_source)) =
            (&self.expected_store, &self.observed_source)
        {
            let expected_states = expected_store.list()?;
            let mut reports = Vec::new();
            let mut failed_devices = Vec::new();
            for expected in &expected_states {
                let observed = match observed_source.get_observed_state(&expected.device_id).await
                {
                    Ok(observed) => observed,
                    Err(_) => {
                        failed_devices.push(expected.device_id.clone());
                        continue;
                    }
                };
                let report = detect_drift(expected, &observed);
                if report.drift_detected {
                    reports.push(report);
                }
            }
            return Ok(DriftAuditRunSummary::from_reports_and_failures(
                expected_states.len(),
                reports,
                failed_devices,
            ));
        }

        Ok(DriftAuditRunSummary::from_reports(
            self.snapshots.len(),
            self.run_snapshot_audit(),
        ))
    }

    fn run_snapshot_audit(&self) -> Vec<DriftReport> {
        self.snapshots
            .iter()
            .map(|snapshot| detect_drift(&snapshot.expected, &snapshot.observed))
            .filter(|report| report.drift_detected)
            .collect()
    }
}

impl DriftAuditWorker {
    pub fn new(auditor: DriftAuditor, event_sink: Arc<dyn EventSink>) -> Self {
        Self {
            auditor,
            event_sink,
            request_id: "drift-audit".into(),
            trace_id: "drift-audit".into(),
        }
    }

    pub fn with_request_context(
        mut self,
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        self.request_id = request_id.into();
        self.trace_id = trace_id.into();
        self
    }

    pub async fn run_once_and_emit(&self) -> UnderlayResult<DriftAuditRunSummary> {
        let summary = self.auditor.run_once_with_summary().await?;
        for report in &summary.reports {
            self.event_sink.emit(UnderlayEvent::drift_detected(
                self.request_id.clone(),
                self.trace_id.clone(),
                report,
            ));
        }
        self.event_sink.emit(UnderlayEvent::drift_audit_completed(
            self.request_id.clone(),
            self.trace_id.clone(),
            summary.audited_devices,
            &summary.drifted_devices,
        ));
        Ok(summary)
    }

    pub async fn run_periodic_until_shutdown<F>(
        &self,
        schedule: DriftAuditSchedule,
        shutdown: F,
    ) -> UnderlayResult<DriftAuditSchedulerReport>
    where
        F: Future<Output = ()>,
    {
        if schedule.interval_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "drift audit schedule interval_secs must be greater than zero".into(),
            ));
        }

        let mut report = DriftAuditSchedulerReport::default();
        if schedule.run_immediately {
            report.last_summary = Some(self.run_once_and_emit().await?);
            report.runs += 1;
        }

        let mut interval = tokio::time::interval(Duration::from_secs(schedule.interval_secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval.tick().await;

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => return Ok(report),
                _ = interval.tick() => {
                    report.last_summary = Some(self.run_once_and_emit().await?);
                    report.runs += 1;
                }
            }
        }
    }
}
