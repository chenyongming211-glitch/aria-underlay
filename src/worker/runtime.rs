use std::future::Future;

use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::worker::drift_auditor::{
    DriftAuditSchedule, DriftAuditSchedulerReport, DriftAuditWorker,
};
use crate::worker::gc::{JournalGcSchedule, JournalGcSchedulerReport, JournalGcWorker};
use crate::worker::operation_alerts::{
    OperationAlertDeliverySchedule, OperationAlertDeliverySchedulerReport,
    OperationAlertDeliveryWorker,
};
use crate::worker::operation_audit_compactor::{
    OperationAuditCompactionSchedule, OperationAuditCompactionSchedulerReport,
    OperationAuditCompactionWorker,
};
use crate::worker::operation_summary_compactor::{
    OperationSummaryCompactionSchedule, OperationSummaryCompactionSchedulerReport,
    OperationSummaryCompactionWorker,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Default)]
pub struct UnderlayWorkerRuntime {
    journal_gc: Option<(JournalGcWorker, JournalGcSchedule)>,
    drift_audit: Option<(DriftAuditWorker, DriftAuditSchedule)>,
    operation_alert_delivery:
        Option<(OperationAlertDeliveryWorker, OperationAlertDeliverySchedule)>,
    operation_summary_compaction:
        Option<(OperationSummaryCompactionWorker, OperationSummaryCompactionSchedule)>,
    operation_audit_compaction:
        Option<(OperationAuditCompactionWorker, OperationAuditCompactionSchedule)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnderlayWorkerRuntimeReport {
    pub journal_gc: Option<JournalGcSchedulerReport>,
    pub drift_audit: Option<DriftAuditSchedulerReport>,
    pub operation_alert_delivery: Option<OperationAlertDeliverySchedulerReport>,
    pub operation_summary_compaction: Option<OperationSummaryCompactionSchedulerReport>,
    pub operation_audit_compaction: Option<OperationAuditCompactionSchedulerReport>,
    pub worker_errors: Vec<String>,
}

enum RuntimeWorkerOutcome {
    JournalGc(UnderlayResult<JournalGcSchedulerReport>),
    DriftAudit(UnderlayResult<DriftAuditSchedulerReport>),
    OperationAlertDelivery(UnderlayResult<OperationAlertDeliverySchedulerReport>),
    OperationSummaryCompaction(UnderlayResult<OperationSummaryCompactionSchedulerReport>),
    OperationAuditCompaction(UnderlayResult<OperationAuditCompactionSchedulerReport>),
}

impl UnderlayWorkerRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_journal_gc(
        mut self,
        worker: JournalGcWorker,
        schedule: JournalGcSchedule,
    ) -> Self {
        self.journal_gc = Some((worker, schedule));
        self
    }

    pub fn with_drift_audit(
        mut self,
        worker: DriftAuditWorker,
        schedule: DriftAuditSchedule,
    ) -> Self {
        self.drift_audit = Some((worker, schedule));
        self
    }

    pub fn with_operation_alert_delivery(
        mut self,
        worker: OperationAlertDeliveryWorker,
        schedule: OperationAlertDeliverySchedule,
    ) -> Self {
        self.operation_alert_delivery = Some((worker, schedule));
        self
    }

    pub fn with_operation_summary_compaction(
        mut self,
        worker: OperationSummaryCompactionWorker,
        schedule: OperationSummaryCompactionSchedule,
    ) -> Self {
        self.operation_summary_compaction = Some((worker, schedule));
        self
    }

    pub fn with_operation_audit_compaction(
        mut self,
        worker: OperationAuditCompactionWorker,
        schedule: OperationAuditCompactionSchedule,
    ) -> Self {
        self.operation_audit_compaction = Some((worker, schedule));
        self
    }

    pub async fn run_until_shutdown<F>(
        self,
        shutdown: F,
    ) -> UnderlayResult<UnderlayWorkerRuntimeReport>
    where
        F: Future<Output = ()>,
    {
        self.validate_schedules()?;

        let mut report = UnderlayWorkerRuntimeReport::default();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut tasks = JoinSet::new();

        if let Some((worker, schedule)) = self.journal_gc {
            let worker_shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                RuntimeWorkerOutcome::JournalGc(
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await,
                )
            });
        }

        if let Some((worker, schedule)) = self.drift_audit {
            let worker_shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                RuntimeWorkerOutcome::DriftAudit(
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await,
                )
            });
        }
        if let Some((worker, schedule)) = self.operation_alert_delivery {
            let worker_shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                RuntimeWorkerOutcome::OperationAlertDelivery(
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await,
                )
            });
        }
        if let Some((worker, schedule)) = self.operation_summary_compaction {
            let worker_shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                RuntimeWorkerOutcome::OperationSummaryCompaction(
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await,
                )
            });
        }
        if let Some((worker, schedule)) = self.operation_audit_compaction {
            let worker_shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                RuntimeWorkerOutcome::OperationAuditCompaction(
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await,
                )
            });
        }
        drop(shutdown_rx);

        if tasks.is_empty() {
            return Ok(report);
        }

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    let _ = shutdown_tx.send(true);
                    while let Some(joined) = tasks.join_next().await {
                        let outcome = joined.map_err(runtime_join_error)?;
                        record_worker_outcome(outcome, &mut report);
                    }
                    return Ok(report);
                }
                joined = tasks.join_next(), if !tasks.is_empty() => {
                    let Some(joined) = joined else {
                        return Ok(report);
                    };
                    match joined {
                        Ok(outcome) => {
                            record_worker_outcome(outcome, &mut report);
                            if tasks.is_empty() {
                                return Ok(report);
                            }
                        }
                        Err(err) => {
                            let _ = shutdown_tx.send(true);
                            drain_workers(&mut tasks).await;
                            return Err(runtime_join_error(err));
                        }
                    }
                }
            }
        }
    }

    fn validate_schedules(&self) -> UnderlayResult<()> {
        if let Some((_, schedule)) = &self.journal_gc {
            validate_interval("journal GC", schedule.interval_secs)?;
        }
        if let Some((_, schedule)) = &self.drift_audit {
            validate_interval("drift audit", schedule.interval_secs)?;
        }
        if let Some((_, schedule)) = &self.operation_alert_delivery {
            validate_interval("operation alert delivery", schedule.interval_secs)?;
        }
        if let Some((_, schedule)) = &self.operation_summary_compaction {
            validate_interval("operation summary compaction", schedule.interval_secs)?;
        }
        if let Some((_, schedule)) = &self.operation_audit_compaction {
            validate_interval("operation audit compaction", schedule.interval_secs)?;
        }
        Ok(())
    }
}

fn validate_interval(worker_name: &str, interval_secs: u64) -> UnderlayResult<()> {
    if interval_secs == 0 {
        return Err(UnderlayError::InvalidIntent(format!(
            "{worker_name} runtime schedule interval_secs must be greater than zero"
        )));
    }
    Ok(())
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

fn record_worker_outcome(
    outcome: RuntimeWorkerOutcome,
    report: &mut UnderlayWorkerRuntimeReport,
) {
    match outcome {
        RuntimeWorkerOutcome::JournalGc(worker_report) => match worker_report {
            Ok(worker_report) => report.journal_gc = Some(worker_report),
            Err(err) => record_worker_error(report, "journal_gc", err),
        },
        RuntimeWorkerOutcome::DriftAudit(worker_report) => match worker_report {
            Ok(worker_report) => report.drift_audit = Some(worker_report),
            Err(err) => record_worker_error(report, "drift_audit", err),
        },
        RuntimeWorkerOutcome::OperationAlertDelivery(worker_report) => match worker_report {
            Ok(worker_report) => report.operation_alert_delivery = Some(worker_report),
            Err(err) => record_worker_error(report, "operation_alert_delivery", err),
        },
        RuntimeWorkerOutcome::OperationSummaryCompaction(worker_report) => match worker_report {
            Ok(worker_report) => report.operation_summary_compaction = Some(worker_report),
            Err(err) => record_worker_error(report, "operation_summary_compaction", err),
        },
        RuntimeWorkerOutcome::OperationAuditCompaction(worker_report) => match worker_report {
            Ok(worker_report) => report.operation_audit_compaction = Some(worker_report),
            Err(err) => record_worker_error(report, "operation_audit_compaction", err),
        },
    }
}

fn record_worker_error(
    report: &mut UnderlayWorkerRuntimeReport,
    worker_name: &str,
    err: UnderlayError,
) {
    report.worker_errors.push(format!("{worker_name}: {err}"));
}

async fn drain_workers(tasks: &mut JoinSet<RuntimeWorkerOutcome>) {
    while tasks.join_next().await.is_some() {}
}

fn runtime_join_error(err: tokio::task::JoinError) -> UnderlayError {
    UnderlayError::Internal(format!("worker runtime task join error: {err}"))
}
