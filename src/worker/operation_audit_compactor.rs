use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::MissedTickBehavior;

use crate::telemetry::{
    JsonFileOperationAuditStore, OperationAuditCompactionReport, OperationAuditRetentionPolicy,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAuditCompactionSchedule {
    pub interval_secs: u64,
    pub run_immediately: bool,
}

impl Default for OperationAuditCompactionSchedule {
    fn default() -> Self {
        Self {
            interval_secs: 60 * 60,
            run_immediately: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAuditCompactionSchedulerReport {
    pub runs: usize,
    pub last_report: Option<OperationAuditCompactionReport>,
}

#[derive(Debug)]
pub struct OperationAuditCompactionWorker {
    store: Arc<JsonFileOperationAuditStore>,
    policy: OperationAuditRetentionPolicy,
}

impl OperationAuditCompactionWorker {
    pub fn new(
        store: Arc<JsonFileOperationAuditStore>,
        policy: OperationAuditRetentionPolicy,
    ) -> Self {
        Self { store, policy }
    }

    pub fn run_once(&self) -> UnderlayResult<OperationAuditCompactionReport> {
        self.store.compact(self.policy.clone())
    }

    pub async fn run_periodic_until_shutdown<F>(
        &self,
        schedule: OperationAuditCompactionSchedule,
        shutdown: F,
    ) -> UnderlayResult<OperationAuditCompactionSchedulerReport>
    where
        F: Future<Output = ()>,
    {
        if schedule.interval_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "operation audit compaction schedule interval_secs must be greater than zero"
                    .into(),
            ));
        }

        let mut report = OperationAuditCompactionSchedulerReport::default();
        if schedule.run_immediately {
            report.last_report = Some(self.run_once()?);
            report.runs += 1;
        }

        let mut interval = tokio::time::interval(Duration::from_secs(schedule.interval_secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval.tick().await;

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => return Ok(report),
                _ = interval.tick() => {
                    report.last_report = Some(self.run_once()?);
                    report.runs += 1;
                }
            }
        }
    }
}
