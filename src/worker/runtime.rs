use std::{collections::BTreeMap, future::Future, time::Duration};

use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::worker::confirmed_commit::{
    ConfirmedCommitTimeoutWatcher, ConfirmedCommitTimeoutWatcherSchedule,
    ConfirmedCommitTimeoutWatcherSchedulerReport,
};
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

const WORKER_RESTART_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
pub struct UnderlayWorkerRuntime {
    journal_gc: Option<(JournalGcWorker, JournalGcSchedule)>,
    confirmed_commit_timeout: Option<(
        ConfirmedCommitTimeoutWatcher,
        ConfirmedCommitTimeoutWatcherSchedule,
    )>,
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
    pub confirmed_commit_timeout: Option<ConfirmedCommitTimeoutWatcherSchedulerReport>,
    pub drift_audit: Option<DriftAuditSchedulerReport>,
    pub operation_alert_delivery: Option<OperationAlertDeliverySchedulerReport>,
    pub operation_summary_compaction: Option<OperationSummaryCompactionSchedulerReport>,
    pub operation_audit_compaction: Option<OperationAuditCompactionSchedulerReport>,
    pub worker_errors: Vec<String>,
}

enum RuntimeWorkerOutcome {
    JournalGc(WorkerRun<JournalGcSchedulerReport>),
    ConfirmedCommitTimeout(WorkerRun<ConfirmedCommitTimeoutWatcherSchedulerReport>),
    DriftAudit(WorkerRun<DriftAuditSchedulerReport>),
    OperationAlertDelivery(WorkerRun<OperationAlertDeliverySchedulerReport>),
    OperationSummaryCompaction(WorkerRun<OperationSummaryCompactionSchedulerReport>),
    OperationAuditCompaction(WorkerRun<OperationAuditCompactionSchedulerReport>),
    RestartSkipped,
}

enum WorkerRun<T> {
    Finished(UnderlayResult<T>),
    Panicked(tokio::task::JoinError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RuntimeWorkerKind {
    JournalGc,
    ConfirmedCommitTimeout,
    DriftAudit,
    OperationAlertDelivery,
    OperationSummaryCompaction,
    OperationAuditCompaction,
}

#[derive(Debug, Clone)]
enum RuntimeWorkerSpec {
    JournalGc(JournalGcWorker, JournalGcSchedule),
    ConfirmedCommitTimeout(
        ConfirmedCommitTimeoutWatcher,
        ConfirmedCommitTimeoutWatcherSchedule,
    ),
    DriftAudit(DriftAuditWorker, DriftAuditSchedule),
    OperationAlertDelivery(
        OperationAlertDeliveryWorker,
        OperationAlertDeliverySchedule,
    ),
    OperationSummaryCompaction(
        OperationSummaryCompactionWorker,
        OperationSummaryCompactionSchedule,
    ),
    OperationAuditCompaction(
        OperationAuditCompactionWorker,
        OperationAuditCompactionSchedule,
    ),
}

impl RuntimeWorkerSpec {
    fn kind(&self) -> RuntimeWorkerKind {
        match self {
            Self::JournalGc(_, _) => RuntimeWorkerKind::JournalGc,
            Self::ConfirmedCommitTimeout(_, _) => RuntimeWorkerKind::ConfirmedCommitTimeout,
            Self::DriftAudit(_, _) => RuntimeWorkerKind::DriftAudit,
            Self::OperationAlertDelivery(_, _) => RuntimeWorkerKind::OperationAlertDelivery,
            Self::OperationSummaryCompaction(_, _) => RuntimeWorkerKind::OperationSummaryCompaction,
            Self::OperationAuditCompaction(_, _) => RuntimeWorkerKind::OperationAuditCompaction,
        }
    }

    async fn run(self, worker_shutdown: watch::Receiver<bool>) -> RuntimeWorkerOutcome {
        match self {
            Self::JournalGc(worker, schedule) => RuntimeWorkerOutcome::JournalGc(
                run_isolated(async move {
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await
                })
                .await,
            ),
            Self::ConfirmedCommitTimeout(worker, schedule) => {
                RuntimeWorkerOutcome::ConfirmedCommitTimeout(
                    run_isolated(async move {
                        worker
                            .run_periodic_until_shutdown(
                                schedule,
                                wait_for_shutdown(worker_shutdown),
                            )
                            .await
                    })
                    .await,
                )
            }
            Self::DriftAudit(worker, schedule) => RuntimeWorkerOutcome::DriftAudit(
                run_isolated(async move {
                    worker
                        .run_periodic_until_shutdown(schedule, wait_for_shutdown(worker_shutdown))
                        .await
                })
                .await,
            ),
            Self::OperationAlertDelivery(worker, schedule) => {
                RuntimeWorkerOutcome::OperationAlertDelivery(
                    run_isolated(async move {
                        worker
                            .run_periodic_until_shutdown(
                                schedule,
                                wait_for_shutdown(worker_shutdown),
                            )
                            .await
                    })
                    .await,
                )
            }
            Self::OperationSummaryCompaction(worker, schedule) => {
                RuntimeWorkerOutcome::OperationSummaryCompaction(
                    run_isolated(async move {
                        worker
                            .run_periodic_until_shutdown(
                                schedule,
                                wait_for_shutdown(worker_shutdown),
                            )
                            .await
                    })
                    .await,
                )
            }
            Self::OperationAuditCompaction(worker, schedule) => {
                RuntimeWorkerOutcome::OperationAuditCompaction(
                    run_isolated(async move {
                        worker
                            .run_periodic_until_shutdown(
                                schedule,
                                wait_for_shutdown(worker_shutdown),
                            )
                            .await
                    })
                    .await,
                )
            }
        }
    }
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

    pub fn with_confirmed_commit_timeout_watcher(
        mut self,
        worker: ConfirmedCommitTimeoutWatcher,
        schedule: ConfirmedCommitTimeoutWatcherSchedule,
    ) -> Self {
        self.confirmed_commit_timeout = Some((worker, schedule));
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
        let mut specs = Vec::new();
        if let Some((worker, schedule)) = self.journal_gc {
            specs.push(RuntimeWorkerSpec::JournalGc(worker, schedule));
        }
        if let Some((worker, schedule)) = self.confirmed_commit_timeout {
            specs.push(RuntimeWorkerSpec::ConfirmedCommitTimeout(
                worker, schedule,
            ));
        }
        if let Some((worker, schedule)) = self.drift_audit {
            specs.push(RuntimeWorkerSpec::DriftAudit(worker, schedule));
        }
        if let Some((worker, schedule)) = self.operation_alert_delivery {
            specs.push(RuntimeWorkerSpec::OperationAlertDelivery(
                worker, schedule,
            ));
        }
        if let Some((worker, schedule)) = self.operation_summary_compaction {
            specs.push(RuntimeWorkerSpec::OperationSummaryCompaction(
                worker, schedule,
            ));
        }
        if let Some((worker, schedule)) = self.operation_audit_compaction {
            specs.push(RuntimeWorkerSpec::OperationAuditCompaction(
                worker, schedule,
            ));
        }
        for spec in &specs {
            spawn_runtime_worker(spec, &shutdown_rx, &mut tasks, None);
        }
        drop(shutdown_rx);
        let mut restart_attempts = BTreeMap::<RuntimeWorkerKind, u32>::new();

        if tasks.is_empty() {
            return Ok(report);
        }

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    let _ = shutdown_tx.send(true);
                    while let Some(joined) = tasks.join_next().await {
                        match joined {
                            Ok(outcome) => {
                                let _ = record_worker_outcome(outcome, &mut report);
                            }
                            Err(err) => record_worker_join_error(&mut report, err),
                        }
                    }
                    return Ok(report);
                }
                joined = tasks.join_next(), if !tasks.is_empty() => {
                    let Some(joined) = joined else {
                        return Ok(report);
                    };
                    match joined {
                        Ok(outcome) => {
                            if let Some(kind) = record_worker_outcome(outcome, &mut report) {
                                if let Some(spec) = specs.iter().find(|spec| spec.kind() == kind) {
                                    let attempts = restart_attempts.entry(kind).or_insert(0);
                                    let restart_delay = worker_restart_delay(*attempts);
                                    *attempts = attempts.saturating_add(1);
                                    spawn_runtime_worker(
                                        spec,
                                        &shutdown_tx.subscribe(),
                                        &mut tasks,
                                        Some(restart_delay),
                                    );
                                }
                            }
                            if tasks.is_empty() {
                                return Ok(report);
                            }
                        }
                        Err(err) => {
                            record_worker_join_error(&mut report, err);
                            if tasks.is_empty() {
                                return Ok(report);
                            }
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
        if let Some((_, schedule)) = &self.confirmed_commit_timeout {
            validate_interval(
                "confirmed-commit timeout watcher",
                schedule.interval_secs,
            )?;
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

fn spawn_runtime_worker(
    spec: &RuntimeWorkerSpec,
    shutdown_rx: &watch::Receiver<bool>,
    tasks: &mut JoinSet<RuntimeWorkerOutcome>,
    restart_delay: Option<Duration>,
) {
    let spec = spec.clone();
    let worker_shutdown = shutdown_rx.clone();
    tasks.spawn(async move {
        if let Some(delay) = restart_delay {
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = wait_for_shutdown(worker_shutdown.clone()) => {
                    return RuntimeWorkerOutcome::RestartSkipped;
                }
            }
        }
        spec.run(worker_shutdown).await
    });
}

async fn run_isolated<T, F>(future: F) -> WorkerRun<T>
where
    T: Send + 'static,
    F: Future<Output = UnderlayResult<T>> + Send + 'static,
{
    match tokio::spawn(future).await {
        Ok(result) => WorkerRun::Finished(result),
        Err(err) => WorkerRun::Panicked(err),
    }
}

fn record_worker_outcome(
    outcome: RuntimeWorkerOutcome,
    report: &mut UnderlayWorkerRuntimeReport,
) -> Option<RuntimeWorkerKind> {
    match outcome {
        RuntimeWorkerOutcome::JournalGc(worker_report) => match worker_report {
            WorkerRun::Finished(Ok(worker_report)) => report.journal_gc = Some(worker_report),
            WorkerRun::Finished(Err(err)) => record_worker_error(report, "journal_gc", err),
            WorkerRun::Panicked(err) => {
                record_worker_join_error_for(report, RuntimeWorkerKind::JournalGc, err);
                return Some(RuntimeWorkerKind::JournalGc);
            }
        },
        RuntimeWorkerOutcome::ConfirmedCommitTimeout(worker_report) => match worker_report {
            WorkerRun::Finished(Ok(worker_report)) => {
                report.confirmed_commit_timeout = Some(worker_report)
            }
            WorkerRun::Finished(Err(err)) => {
                record_worker_error(report, "confirmed_commit_timeout", err)
            }
            WorkerRun::Panicked(err) => {
                record_worker_join_error_for(
                    report,
                    RuntimeWorkerKind::ConfirmedCommitTimeout,
                    err,
                );
                return Some(RuntimeWorkerKind::ConfirmedCommitTimeout);
            }
        },
        RuntimeWorkerOutcome::DriftAudit(worker_report) => match worker_report {
            WorkerRun::Finished(Ok(worker_report)) => report.drift_audit = Some(worker_report),
            WorkerRun::Finished(Err(err)) => record_worker_error(report, "drift_audit", err),
            WorkerRun::Panicked(err) => {
                record_worker_join_error_for(report, RuntimeWorkerKind::DriftAudit, err);
                return Some(RuntimeWorkerKind::DriftAudit);
            }
        },
        RuntimeWorkerOutcome::OperationAlertDelivery(worker_report) => match worker_report {
            WorkerRun::Finished(Ok(worker_report)) => {
                report.operation_alert_delivery = Some(worker_report)
            }
            WorkerRun::Finished(Err(err)) => {
                record_worker_error(report, "operation_alert_delivery", err)
            }
            WorkerRun::Panicked(err) => {
                record_worker_join_error_for(
                    report,
                    RuntimeWorkerKind::OperationAlertDelivery,
                    err,
                );
                return Some(RuntimeWorkerKind::OperationAlertDelivery);
            }
        },
        RuntimeWorkerOutcome::OperationSummaryCompaction(worker_report) => match worker_report {
            WorkerRun::Finished(Ok(worker_report)) => {
                report.operation_summary_compaction = Some(worker_report)
            }
            WorkerRun::Finished(Err(err)) => {
                record_worker_error(report, "operation_summary_compaction", err)
            }
            WorkerRun::Panicked(err) => {
                record_worker_join_error_for(
                    report,
                    RuntimeWorkerKind::OperationSummaryCompaction,
                    err,
                );
                return Some(RuntimeWorkerKind::OperationSummaryCompaction);
            }
        },
        RuntimeWorkerOutcome::OperationAuditCompaction(worker_report) => match worker_report {
            WorkerRun::Finished(Ok(worker_report)) => {
                report.operation_audit_compaction = Some(worker_report)
            }
            WorkerRun::Finished(Err(err)) => {
                record_worker_error(report, "operation_audit_compaction", err)
            }
            WorkerRun::Panicked(err) => {
                record_worker_join_error_for(
                    report,
                    RuntimeWorkerKind::OperationAuditCompaction,
                    err,
                );
                return Some(RuntimeWorkerKind::OperationAuditCompaction);
            }
        },
        RuntimeWorkerOutcome::RestartSkipped => {}
    }
    None
}

fn record_worker_error(
    report: &mut UnderlayWorkerRuntimeReport,
    worker_name: &str,
    err: UnderlayError,
) {
    report.worker_errors.push(format!("{worker_name}: {err}"));
}

fn record_worker_join_error(
    report: &mut UnderlayWorkerRuntimeReport,
    err: tokio::task::JoinError,
) {
    record_worker_error(report, "worker_runtime", runtime_join_error(err));
}

fn record_worker_join_error_for(
    report: &mut UnderlayWorkerRuntimeReport,
    kind: RuntimeWorkerKind,
    err: tokio::task::JoinError,
) {
    record_worker_error(
        report,
        "worker_runtime",
        UnderlayError::Internal(format!(
            "{} task join error: {err}",
            runtime_worker_name(kind)
        )),
    );
}

fn runtime_worker_name(kind: RuntimeWorkerKind) -> &'static str {
    match kind {
        RuntimeWorkerKind::JournalGc => "journal_gc",
        RuntimeWorkerKind::ConfirmedCommitTimeout => "confirmed_commit_timeout",
        RuntimeWorkerKind::DriftAudit => "drift_audit",
        RuntimeWorkerKind::OperationAlertDelivery => "operation_alert_delivery",
        RuntimeWorkerKind::OperationSummaryCompaction => "operation_summary_compaction",
        RuntimeWorkerKind::OperationAuditCompaction => "operation_audit_compaction",
    }
}

fn worker_restart_delay(previous_restarts: u32) -> Duration {
    let multiplier = 2_u64.saturating_pow(previous_restarts.min(6));
    WORKER_RESTART_DELAY
        .saturating_mul(multiplier as u32)
        .min(Duration::from_secs(5))
}

fn runtime_join_error(err: tokio::task::JoinError) -> UnderlayError {
    UnderlayError::Internal(format!("worker runtime task join error: {err}"))
}
