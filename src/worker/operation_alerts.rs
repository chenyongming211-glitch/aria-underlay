use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::MissedTickBehavior;

use crate::telemetry::{
    OperationAlert, OperationAlertCheckpointStore, OperationAlertSink, OperationSummaryStore,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAlertDeliverySchedule {
    pub interval_secs: u64,
    pub run_immediately: bool,
}

impl Default for OperationAlertDeliverySchedule {
    fn default() -> Self {
        Self {
            interval_secs: 5 * 60,
            run_immediately: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAlertDeliveryReport {
    pub scanned_attention_required: usize,
    pub already_delivered: usize,
    pub newly_delivered: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAlertDeliverySchedulerReport {
    pub runs: usize,
    pub last_report: Option<OperationAlertDeliveryReport>,
}

#[derive(Debug)]
pub struct OperationAlertDeliveryWorker {
    operation_summaries: Arc<dyn OperationSummaryStore>,
    alert_sink: Arc<dyn OperationAlertSink>,
    checkpoint: Arc<dyn OperationAlertCheckpointStore>,
}

impl OperationAlertDeliveryWorker {
    pub fn new(
        operation_summaries: Arc<dyn OperationSummaryStore>,
        alert_sink: Arc<dyn OperationAlertSink>,
        checkpoint: Arc<dyn OperationAlertCheckpointStore>,
    ) -> Self {
        Self {
            operation_summaries,
            alert_sink,
            checkpoint,
        }
    }

    pub fn run_once(&self) -> UnderlayResult<OperationAlertDeliveryReport> {
        let summaries = self.operation_summaries.list_attention_required()?;
        let delivered_keys = self.checkpoint.delivered_keys()?;
        let scanned_attention_required = summaries.len();
        let mut already_delivered = 0;
        let mut alerts = Vec::new();

        for summary in summaries {
            let alert = OperationAlert::from_summary(summary);
            if delivered_keys.contains(&alert.dedupe_key) {
                already_delivered += 1;
            } else {
                alerts.push(alert);
            }
        }

        let newly_delivered = alerts.len();
        if newly_delivered > 0 {
            self.alert_sink.deliver(&alerts)?;
            let keys = alerts
                .iter()
                .map(|alert| alert.dedupe_key.clone())
                .collect::<Vec<_>>();
            self.checkpoint.record_delivered(&keys)?;
        }

        Ok(OperationAlertDeliveryReport {
            scanned_attention_required,
            already_delivered,
            newly_delivered,
        })
    }

    pub async fn run_periodic_until_shutdown<F>(
        &self,
        schedule: OperationAlertDeliverySchedule,
        shutdown: F,
    ) -> UnderlayResult<OperationAlertDeliverySchedulerReport>
    where
        F: Future<Output = ()>,
    {
        if schedule.interval_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "operation alert delivery schedule interval_secs must be greater than zero".into(),
            ));
        }

        let mut report = OperationAlertDeliverySchedulerReport::default();
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
