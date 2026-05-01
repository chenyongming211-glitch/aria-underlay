use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::OperationSummary;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListOperationSummariesRequest {
    pub attention_required_only: bool,
    pub action: Option<String>,
    pub device_id: Option<DeviceId>,
    pub tx_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOperationSummariesResponse {
    pub summaries: Vec<OperationSummary>,
}
