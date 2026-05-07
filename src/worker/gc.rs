use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

use crate::telemetry::{EventSink, UnderlayEvent};
use crate::tx::{TxJournalRecord, TxPhase};
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl RetentionPolicy {
    pub fn validate(&self) -> UnderlayResult<()> {
        if self.max_artifacts_per_device == 0 {
            return Err(UnderlayError::InvalidIntent(
                "journal GC retention max_artifacts_per_device must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct JournalGc {
    journal_root: Option<PathBuf>,
    artifact_root: Option<PathBuf>,
    now_unix_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalGcReport {
    pub journals_deleted: usize,
    pub journals_retained: usize,
    pub journals_failed: usize,
    pub artifacts_deleted: usize,
    pub journal_deleted_tx_ids: Vec<String>,
    pub failed_journal_refs: Vec<String>,
    pub artifact_deleted_refs: Vec<String>,
}

impl JournalGcReport {
    pub fn deleted_total(&self) -> usize {
        self.journals_deleted + self.artifacts_deleted
    }

    fn sort_details(&mut self) {
        self.journal_deleted_tx_ids.sort();
        self.failed_journal_refs.sort();
        self.artifact_deleted_refs.sort();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalGcSchedule {
    pub interval_secs: u64,
    pub run_immediately: bool,
}

impl Default for JournalGcSchedule {
    fn default() -> Self {
        Self {
            interval_secs: 60 * 60,
            run_immediately: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalGcSchedulerReport {
    pub runs: usize,
    pub last_report: Option<JournalGcReport>,
}

#[derive(Debug)]
pub struct JournalGcWorker {
    gc: JournalGc,
    policy: RetentionPolicy,
    event_sink: Arc<dyn EventSink>,
    request_id: String,
    trace_id: String,
}

impl JournalGcWorker {
    pub fn new(
        gc: JournalGc,
        policy: RetentionPolicy,
        event_sink: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            gc,
            policy,
            event_sink,
            request_id: "journal-gc".into(),
            trace_id: "journal-gc".into(),
        }
    }

    pub fn with_request_context(
        mut self,
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        self.request_id = request_id.into();
        self.trace_id = trace_id.into();
        self
    }

    pub async fn run_once_and_emit(&self) -> UnderlayResult<JournalGcReport> {
        let report = self.gc.run_once(self.policy.clone()).await?;
        self.event_sink.emit(UnderlayEvent::journal_gc_completed(
            self.request_id.clone(),
            self.trace_id.clone(),
            &report,
        ));
        Ok(report)
    }

    pub async fn run_periodic_until_shutdown<F>(
        &self,
        schedule: JournalGcSchedule,
        shutdown: F,
    ) -> UnderlayResult<JournalGcSchedulerReport>
    where
        F: Future<Output = ()>,
    {
        if schedule.interval_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "journal GC schedule interval_secs must be greater than zero".into(),
            ));
        }

        let mut summary = JournalGcSchedulerReport::default();
        if schedule.run_immediately {
            summary.last_report = Some(self.run_once_and_emit().await?);
            summary.runs += 1;
        }

        let mut interval = tokio::time::interval(Duration::from_secs(schedule.interval_secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval.tick().await;

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => return Ok(summary),
                _ = interval.tick() => {
                    summary.last_report = Some(self.run_once_and_emit().await?);
                    summary.runs += 1;
                }
            }
        }
    }
}

impl JournalGc {
    pub fn new(journal_root: impl Into<PathBuf>) -> Self {
        Self {
            journal_root: Some(journal_root.into()),
            artifact_root: None,
            now_unix_secs: None,
        }
    }

    pub fn with_artifact_root(mut self, artifact_root: impl Into<PathBuf>) -> Self {
        self.artifact_root = Some(artifact_root.into());
        self
    }

    pub fn with_now_unix_secs(mut self, now_unix_secs: u64) -> Self {
        self.now_unix_secs = Some(now_unix_secs);
        self
    }

    pub async fn run_once(&self, policy: RetentionPolicy) -> UnderlayResult<JournalGcReport> {
        let Some(journal_root) = &self.journal_root else {
            return Ok(JournalGcReport::default());
        };
        if !journal_root.exists() {
            return Ok(JournalGcReport::default());
        }

        let now = self.now_unix_secs.unwrap_or_else(now_unix_secs);
        let mut report = JournalGcReport::default();
        let mut terminal_records = Vec::new();

        for entry in fs::read_dir(journal_root).map_err(gc_io_error)? {
            let path = entry.map_err(gc_io_error)?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let record = match read_journal_record(&path) {
                Ok(record) => record,
                Err(_) => {
                    report.journals_failed += 1;
                    report.failed_journal_refs.push(path_ref(&path));
                    continue;
                }
            };
            if !is_terminal_phase(&record.phase) {
                report.journals_retained += 1;
                continue;
            }

            let artifact_due = is_older_than(
                record.updated_at_unix_secs,
                now,
                policy.rollback_artifact_retention_days,
            );
            let journal_due = is_older_than(
                record.updated_at_unix_secs,
                now,
                journal_retention_days(&record.phase, &policy)
                    .max(policy.rollback_artifact_retention_days),
            );

            terminal_records.push(record.clone());
            if journal_due {
                fs::remove_file(&path).map_err(gc_io_error)?;
                report.journals_deleted += 1;
                report.journal_deleted_tx_ids.push(record.tx_id.clone());
            } else {
                report.journals_retained += 1;
            }

            if artifact_due {
                let deleted_refs = self.delete_artifacts_for_tx(&record.tx_id)?;
                report.artifacts_deleted += deleted_refs.len();
                report.artifact_deleted_refs.extend(deleted_refs);
            }
        }

        let pruned_refs = self.prune_artifacts_per_device(
            &terminal_records,
            policy.max_artifacts_per_device as usize,
        )?;
        report.artifacts_deleted += pruned_refs.len();
        report.artifact_deleted_refs.extend(pruned_refs);
        report.sort_details();
        Ok(report)
    }

    fn delete_artifacts_for_tx(&self, tx_id: &str) -> UnderlayResult<Vec<String>> {
        let Some(artifact_root) = &self.artifact_root else {
            return Ok(Vec::new());
        };
        if !artifact_root.exists() {
            return Ok(Vec::new());
        }

        let mut deleted = Vec::new();
        for entry in fs::read_dir(artifact_root).map_err(gc_io_error)? {
            let device_dir = entry.map_err(gc_io_error)?.path();
            if !device_dir.is_dir() {
                continue;
            }
            let Some(device_id) = device_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let tx_dir = device_dir.join(tx_id);
            if tx_dir.is_dir() {
                fs::remove_dir_all(&tx_dir).map_err(gc_io_error)?;
                deleted.push(format!("{device_id}/{tx_id}"));
            }
        }
        Ok(deleted)
    }

    fn prune_artifacts_per_device(
        &self,
        terminal_records: &[TxJournalRecord],
        max_artifacts_per_device: usize,
    ) -> UnderlayResult<Vec<String>> {
        let Some(artifact_root) = &self.artifact_root else {
            return Ok(Vec::new());
        };
        if max_artifacts_per_device == 0 || !artifact_root.exists() {
            return Ok(Vec::new());
        }

        let terminal_by_tx = terminal_records
            .iter()
            .filter(|record| is_terminal_phase(&record.phase))
            .map(|record| (record.tx_id.as_str(), record.updated_at_unix_secs))
            .collect::<BTreeMap<_, _>>();

        let mut deleted = Vec::new();
        for entry in fs::read_dir(artifact_root).map_err(gc_io_error)? {
            let device_dir = entry.map_err(gc_io_error)?.path();
            if !device_dir.is_dir() {
                continue;
            }
            let Some(device_id) = device_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let mut terminal_artifacts = Vec::new();
            for tx_entry in fs::read_dir(&device_dir).map_err(gc_io_error)? {
                let tx_dir = tx_entry.map_err(gc_io_error)?.path();
                if !tx_dir.is_dir() {
                    continue;
                }
                let Some(tx_id) = tx_dir.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some(updated_at) = terminal_by_tx.get(tx_id) else {
                    continue;
                };
                terminal_artifacts.push((*updated_at, tx_id.to_string(), tx_dir));
            }

            terminal_artifacts.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| right.1.cmp(&left.1))
            });

            let mut kept = BTreeSet::new();
            for (_, tx_id, tx_dir) in terminal_artifacts {
                if kept.len() < max_artifacts_per_device {
                    kept.insert(tx_id);
                    continue;
                }
                if tx_dir.exists() {
                    fs::remove_dir_all(&tx_dir).map_err(gc_io_error)?;
                    deleted.push(format!("{device_id}/{tx_id}"));
                }
            }
        }
        Ok(deleted)
    }
}

fn read_journal_record(path: &Path) -> UnderlayResult<TxJournalRecord> {
    let payload = fs::read(path).map_err(gc_io_error)?;
    serde_json::from_slice(&payload)
        .map_err(|err| UnderlayError::Internal(format!("parse tx journal {:?}: {err}", path)))
}

fn path_ref(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn is_terminal_phase(phase: &TxPhase) -> bool {
    matches!(
        phase,
        TxPhase::Committed | TxPhase::RolledBack | TxPhase::Failed | TxPhase::ForceResolved
    )
}

fn journal_retention_days(phase: &TxPhase, policy: &RetentionPolicy) -> u32 {
    match phase {
        TxPhase::Committed => policy.committed_journal_retention_days,
        TxPhase::RolledBack => policy.rolled_back_journal_retention_days,
        TxPhase::Failed | TxPhase::ForceResolved => policy.failed_journal_retention_days,
        _ => u32::MAX,
    }
}

fn is_older_than(updated_at_unix_secs: u64, now_unix_secs: u64, retention_days: u32) -> bool {
    if retention_days == u32::MAX {
        return false;
    }
    let retention_secs = u64::from(retention_days).saturating_mul(24 * 60 * 60);
    now_unix_secs.saturating_sub(updated_at_unix_secs) >= retention_secs
}

fn gc_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("journal gc io error: {err}"))
}
