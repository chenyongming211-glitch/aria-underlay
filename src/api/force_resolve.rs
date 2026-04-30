use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::tx::TxPhase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForceResolveTransactionRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub tx_id: String,
    pub operator: String,
    pub reason: String,
    pub break_glass_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForceResolveTransactionResponse {
    pub tx_id: String,
    pub previous_phase: TxPhase,
    pub resolved_phase: TxPhase,
    pub devices: Vec<DeviceId>,
    pub resolved: bool,
    pub warnings: Vec<String>,
}
