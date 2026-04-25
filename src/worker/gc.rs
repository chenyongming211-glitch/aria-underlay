use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub committed_journal_retention_days: u32,
    pub rolled_back_journal_retention_days: u32,
    pub failed_journal_retention_days: u32,
    pub rollback_artifact_retention_days: u32,
    pub max_artifacts_per_device: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            committed_journal_retention_days: 30,
            rolled_back_journal_retention_days: 30,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 50,
        }
    }
}

#[derive(Debug, Default)]
pub struct JournalGc;

impl JournalGc {
    pub async fn run_once(&self, _policy: RetentionPolicy) {}
}

