use crate::state::drift::DriftReport;

#[derive(Debug, Default)]
pub struct DriftAuditor;

impl DriftAuditor {
    pub async fn run_once(&self) -> Vec<DriftReport> {
        Vec::new()
    }
}

