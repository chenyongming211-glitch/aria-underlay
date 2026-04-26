use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxContext {
    pub tx_id: String,
    pub request_id: String,
    pub trace_id: String,
}

impl TxContext {
    pub fn new(request_id: String, trace_id: String) -> Self {
        Self {
            tx_id: uuid::Uuid::new_v4().to_string(),
            request_id,
            trace_id,
        }
    }
}
