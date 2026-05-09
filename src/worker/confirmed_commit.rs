use std::future::Future;
use std::time::Duration;

use tokio::time::MissedTickBehavior;

use crate::api::AriaUnderlayService;
use crate::tx::recovery::RecoveryReport;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedCommitTimeoutWatcherSchedule {
    pub interval_secs: u64,
    pub run_immediately: bool,
}

impl Default for ConfirmedCommitTimeoutWatcherSchedule {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            run_immediately: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfirmedCommitTimeoutWatcherSchedulerReport {
    pub runs: usize,
    pub last_report: Option<RecoveryReport>,
}

#[derive(Debug, Clone)]
pub struct ConfirmedCommitTimeoutWatcher {
    service: AriaUnderlayService,
}

impl ConfirmedCommitTimeoutWatcher {
    pub fn new(service: AriaUnderlayService) -> Self {
        Self { service }
    }

    pub async fn run_once(&self) -> UnderlayResult<RecoveryReport> {
        self.service.recover_timed_out_confirmed_commits().await
    }

    pub async fn run_periodic_until_shutdown<F>(
        &self,
        schedule: ConfirmedCommitTimeoutWatcherSchedule,
        shutdown: F,
    ) -> UnderlayResult<ConfirmedCommitTimeoutWatcherSchedulerReport>
    where
        F: Future<Output = ()>,
    {
        if schedule.interval_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "confirmed-commit timeout watcher schedule interval_secs must be greater than zero"
                    .into(),
            ));
        }

        let mut report = ConfirmedCommitTimeoutWatcherSchedulerReport::default();
        if schedule.run_immediately {
            report.last_report = Some(self.run_once().await?);
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
                    report.last_report = Some(self.run_once().await?);
                    report.runs += 1;
                }
            }
        }
    }
}
