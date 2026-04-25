use serde::{Deserialize, Serialize};

use crate::model::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxPhase {
    Started,
    Preparing,
    Prepared,
    Committing,
    Verifying,
    Committed,
    RollingBack,
    RolledBack,
    Failed,
    InDoubt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxJournalRecord {
    pub tx_id: String,
    pub request_id: String,
    pub trace_id: String,
    pub phase: TxPhase,
    pub devices: Vec<DeviceId>,
}

