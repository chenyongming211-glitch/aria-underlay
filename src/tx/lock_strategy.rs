use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockAcquisitionPolicy {
    pub max_wait_secs: u64,
    pub initial_delay_ms: u64,
    pub max_delay_secs: u64,
    pub jitter: bool,
    pub force_unlock_enabled: bool,
}

impl Default for LockAcquisitionPolicy {
    fn default() -> Self {
        Self {
            max_wait_secs: 30,
            initial_delay_ms: 500,
            max_delay_secs: 5,
            jitter: true,
            force_unlock_enabled: false,
        }
    }
}

impl LockAcquisitionPolicy {
    pub fn initial_delay(&self) -> Duration {
        Duration::from_millis(self.initial_delay_ms)
    }
}

