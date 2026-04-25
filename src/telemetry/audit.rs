#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub request_id: String,
    pub trace_id: String,
    pub action: String,
}

