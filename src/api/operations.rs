use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::OperationSummary;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListOperationSummariesRequest {
    pub attention_required_only: bool,
    pub action: Option<String>,
    pub result: Option<String>,
    pub device_id: Option<DeviceId>,
    pub tx_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOperationSummariesResponse {
    pub summaries: Vec<OperationSummary>,
    pub overview: OperationSummaryOverview,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummaryOverview {
    pub matched_records: usize,
    pub returned_records: usize,
    pub attention_required: usize,
    pub by_action: BTreeMap<String, usize>,
    pub by_result: BTreeMap<String, usize>,
    pub by_device: BTreeMap<String, usize>,
}

impl OperationSummaryOverview {
    pub fn from_summaries(summaries: &[OperationSummary], returned_records: usize) -> Self {
        let mut overview = Self {
            matched_records: summaries.len(),
            returned_records,
            ..Default::default()
        };

        for summary in summaries {
            if summary.attention_required {
                overview.attention_required += 1;
            }
            increment(&mut overview.by_action, &summary.action);
            increment(&mut overview.by_result, &summary.result);
            if let Some(device_id) = &summary.device_id {
                increment(&mut overview.by_device, &device_id.0);
            }
        }

        overview
    }
}

fn increment(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}
