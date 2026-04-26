use crate::state::drift::{detect_drift, DriftReport};
use crate::state::DeviceShadowState;

#[derive(Debug, Default)]
pub struct DriftAuditor {
    snapshots: Vec<DriftAuditSnapshot>,
}

#[derive(Debug, Clone)]
pub struct DriftAuditSnapshot {
    pub expected: DeviceShadowState,
    pub observed: DeviceShadowState,
}

impl DriftAuditor {
    pub fn new(snapshots: Vec<DriftAuditSnapshot>) -> Self {
        Self { snapshots }
    }

    pub async fn run_once(&self) -> Vec<DriftReport> {
        self.snapshots
            .iter()
            .map(|snapshot| detect_drift(&snapshot.expected, &snapshot.observed))
            .filter(|report| report.drift_detected)
            .collect()
    }
}
