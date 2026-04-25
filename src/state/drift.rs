use serde::{Deserialize, Serialize};

use crate::model::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftPolicy {
    ReportOnly,
    BlockNewTransaction,
    AutoReconcile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    pub device_id: DeviceId,
    pub drift_detected: bool,
}

