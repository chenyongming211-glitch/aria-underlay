use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::tx::journal::TxJournalErrorEvent;
use crate::tx::{TransactionStrategy, TxPhase};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListInDoubtTransactionsRequest {
    pub device_id: Option<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListInDoubtTransactionsResponse {
    pub transactions: Vec<InDoubtTransactionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InDoubtTransactionSummary {
    pub tx_id: String,
    pub request_id: String,
    pub trace_id: String,
    pub phase: TxPhase,
    pub devices: Vec<DeviceId>,
    pub strategy: Option<TransactionStrategy>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error_history: Vec<TxJournalErrorEvent>,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}
