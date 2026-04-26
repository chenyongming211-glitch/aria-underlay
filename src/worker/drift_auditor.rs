use std::sync::Arc;

use async_trait::async_trait;

use crate::model::DeviceId;
use crate::state::drift::{detect_drift, DriftReport};
use crate::state::{DeviceShadowState, ShadowStateStore};
use crate::UnderlayResult;

#[derive(Debug, Default)]
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
        if let (Some(expected_store), Some(observed_source)) =
            (&self.expected_store, &self.observed_source)
        {
            let mut reports = Vec::new();
            for expected in expected_store.list()? {
                let observed = observed_source
                    .get_observed_state(&expected.device_id)
                    .await?;
                let report = detect_drift(&expected, &observed);
                if report.drift_detected {
                    reports.push(report);
                }
            }
            return Ok(reports);
        }

        Ok(self.run_snapshot_audit())
    }

    fn run_snapshot_audit(&self) -> Vec<DriftReport> {
        self.snapshots
            .iter()
            .map(|snapshot| detect_drift(&snapshot.expected, &snapshot.observed))
            .filter(|report| report.drift_detected)
            .collect()
    }
}
