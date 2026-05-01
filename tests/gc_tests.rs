use std::fs;
use std::sync::Arc;

use aria_underlay::model::DeviceId;
use aria_underlay::telemetry::{InMemoryEventSink, UnderlayEvent, UnderlayEventKind};
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::worker::gc::{
    JournalGc, JournalGcReport, JournalGcSchedule, JournalGcWorker, RetentionPolicy,
};

#[test]
fn retention_policy_defaults_are_conservative() {
    let policy = RetentionPolicy::default();
    assert_eq!(policy.failed_journal_retention_days, 90);
    assert_eq!(policy.max_artifacts_per_device, 50);
}

#[test]
fn journal_gc_completed_event_includes_cleanup_counts() {
    let report = JournalGcReport {
        journals_deleted: 2,
        journals_retained: 3,
        artifacts_deleted: 4,
        journal_deleted_tx_ids: vec!["tx-old".into()],
        artifact_deleted_refs: vec!["leaf-a/tx-old".into()],
    };

    let event = UnderlayEvent::journal_gc_completed("req-gc", "trace-gc", &report);

    assert_eq!(event.kind, UnderlayEventKind::UnderlayJournalGcCompleted);
    assert_eq!(event.request_id, "req-gc");
    assert_eq!(event.trace_id, "trace-gc");
    assert_eq!(
        event.fields.get("journals_deleted").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        event.fields.get("journals_retained").map(String::as_str),
        Some("3")
    );
    assert_eq!(
        event.fields.get("artifacts_deleted").map(String::as_str),
        Some("4")
    );
    assert_eq!(
        event.fields.get("deleted_total").map(String::as_str),
        Some("6")
    );
    assert_eq!(
        event
            .fields
            .get("journal_deleted_tx_ids")
            .map(String::as_str),
        Some("tx-old")
    );
    assert_eq!(
        event
            .fields
            .get("artifact_deleted_refs")
            .map(String::as_str),
        Some("leaf-a/tx-old")
    );
}

#[tokio::test]
async fn gc_deletes_old_terminal_journal_but_keeps_in_doubt() {
    let temp = temp_test_dir("journal-retention");
    let journal_root = temp.join("journal");
    let store = JsonFileTxJournalStore::new(&journal_root);
    let old_committed = journal_record("tx-old-committed", TxPhase::Committed, 100);
    let old_in_doubt = journal_record("tx-old-in-doubt", TxPhase::InDoubt, 100);
    store.put(&old_committed).expect("write committed");
    store.put(&old_in_doubt).expect("write in doubt");

    let report = JournalGc::new(&journal_root)
        .with_now_unix_secs(100 + 31 * 24 * 60 * 60)
        .run_once(RetentionPolicy {
            committed_journal_retention_days: 30,
            rolled_back_journal_retention_days: 30,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 50,
        })
        .await
        .expect("gc should run");

    assert_eq!(report.journals_deleted, 1);
    assert_eq!(report.journal_deleted_tx_ids, vec!["tx-old-committed".to_string()]);
    assert!(store.get("tx-old-committed").expect("read committed").is_none());
    assert!(store.get("tx-old-in-doubt").expect("read in doubt").is_some());
    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn gc_deletes_artifacts_only_for_old_terminal_transactions() {
    let temp = temp_test_dir("artifact-retention");
    let journal_root = temp.join("journal");
    let artifact_root = temp.join("artifacts");
    let store = JsonFileTxJournalStore::new(&journal_root);
    store
        .put(&journal_record("tx-terminal", TxPhase::Committed, 100))
        .expect("write terminal");
    store
        .put(&journal_record("tx-in-doubt", TxPhase::InDoubt, 100))
        .expect("write in doubt");
    write_artifact(&artifact_root, "leaf-a", "tx-terminal");
    write_artifact(&artifact_root, "leaf-a", "tx-in-doubt");

    let report = JournalGc::new(&journal_root)
        .with_artifact_root(&artifact_root)
        .with_now_unix_secs(100 + 31 * 24 * 60 * 60)
        .run_once(RetentionPolicy {
            committed_journal_retention_days: 90,
            rolled_back_journal_retention_days: 90,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 50,
        })
        .await
        .expect("gc should run");

    assert_eq!(report.artifacts_deleted, 1);
    assert_eq!(
        report.artifact_deleted_refs,
        vec!["leaf-a/tx-terminal".to_string()]
    );
    assert!(!artifact_root.join("leaf-a/tx-terminal").exists());
    assert!(artifact_root.join("leaf-a/tx-in-doubt").exists());
    assert!(store.get("tx-terminal").expect("read terminal").is_some());
    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn gc_prunes_terminal_artifacts_per_device_without_touching_unknown_tx() {
    let temp = temp_test_dir("artifact-cap");
    let journal_root = temp.join("journal");
    let artifact_root = temp.join("artifacts");
    let store = JsonFileTxJournalStore::new(&journal_root);
    store
        .put(&journal_record("tx-new", TxPhase::Committed, 300))
        .expect("write new");
    store
        .put(&journal_record("tx-old", TxPhase::Committed, 200))
        .expect("write old");
    write_artifact(&artifact_root, "leaf-a", "tx-new");
    write_artifact(&artifact_root, "leaf-a", "tx-old");
    write_artifact(&artifact_root, "leaf-a", "tx-unknown");

    let report = JournalGc::new(&journal_root)
        .with_artifact_root(&artifact_root)
        .with_now_unix_secs(301)
        .run_once(RetentionPolicy {
            committed_journal_retention_days: 30,
            rolled_back_journal_retention_days: 30,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 1,
        })
        .await
        .expect("gc should run");

    assert_eq!(report.artifacts_deleted, 1);
    assert_eq!(
        report.artifact_deleted_refs,
        vec!["leaf-a/tx-old".to_string()]
    );
    assert!(artifact_root.join("leaf-a/tx-new").exists());
    assert!(!artifact_root.join("leaf-a/tx-old").exists());
    assert!(artifact_root.join("leaf-a/tx-unknown").exists());
    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn gc_worker_emits_completion_event_after_successful_run() {
    let temp = temp_test_dir("worker-event");
    let journal_root = temp.join("journal");
    let store = JsonFileTxJournalStore::new(&journal_root);
    store
        .put(&journal_record("tx-old", TxPhase::Committed, 100))
        .expect("write committed");
    let sink = Arc::new(InMemoryEventSink::default());

    let report = JournalGcWorker::new(
        JournalGc::new(&journal_root).with_now_unix_secs(100 + 31 * 24 * 60 * 60),
        RetentionPolicy {
            committed_journal_retention_days: 30,
            rolled_back_journal_retention_days: 30,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 50,
        },
        sink.clone(),
    )
    .with_request_context("req-gc", "trace-gc")
    .run_once_and_emit()
    .await
    .expect("worker gc should run");

    assert_eq!(report.journals_deleted, 1);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayJournalGcCompleted);
    assert_eq!(events[0].request_id, "req-gc");
    assert_eq!(events[0].trace_id, "trace-gc");
    assert_eq!(
        events[0].fields.get("journals_deleted").map(String::as_str),
        Some("1")
    );
    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn gc_worker_periodic_runner_runs_immediate_cycle_and_stops_on_shutdown() {
    let temp = temp_test_dir("worker-periodic");
    let journal_root = temp.join("journal");
    let store = JsonFileTxJournalStore::new(&journal_root);
    store
        .put(&journal_record("tx-old", TxPhase::Committed, 100))
        .expect("write committed");
    let sink = Arc::new(InMemoryEventSink::default());
    let worker = JournalGcWorker::new(
        JournalGc::new(&journal_root).with_now_unix_secs(100 + 31 * 24 * 60 * 60),
        RetentionPolicy {
            committed_journal_retention_days: 30,
            rolled_back_journal_retention_days: 30,
            failed_journal_retention_days: 90,
            rollback_artifact_retention_days: 30,
            max_artifacts_per_device: 50,
        },
        sink.clone(),
    );

    let summary = worker
        .run_periodic_until_shutdown(
            JournalGcSchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
            async {},
        )
        .await
        .expect("periodic worker should stop cleanly on shutdown");

    assert_eq!(summary.runs, 1);
    assert_eq!(
        summary
            .last_report
            .as_ref()
            .expect("periodic worker should retain last report")
            .journal_deleted_tx_ids,
        vec!["tx-old".to_string()]
    );
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayJournalGcCompleted);
    fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn gc_worker_rejects_zero_second_periodic_interval() {
    let temp = temp_test_dir("worker-invalid-interval");
    let sink = Arc::new(InMemoryEventSink::default());
    let worker = JournalGcWorker::new(
        JournalGc::new(temp.join("journal")),
        RetentionPolicy::default(),
        sink,
    );

    let err = worker
        .run_periodic_until_shutdown(
            JournalGcSchedule {
                interval_secs: 0,
                run_immediately: false,
            },
            async {},
        )
        .await
        .expect_err("zero interval should fail closed");
    let message = format!("{err}");

    assert!(
        message.contains("interval_secs"),
        "unexpected interval validation error: {message}"
    );
    fs::remove_dir_all(temp).ok();
}

fn journal_record(tx_id: &str, phase: TxPhase, updated_at_unix_secs: u64) -> TxJournalRecord {
    TxJournalRecord {
        tx_id: tx_id.into(),
        request_id: format!("req-{tx_id}"),
        trace_id: format!("trace-{tx_id}"),
        phase,
        devices: vec![DeviceId("leaf-a".into())],
        desired_states: Vec::new(),
        change_sets: Vec::new(),
        strategy: None,
        error_code: None,
        error_message: None,
        error_history: Vec::new(),
        manual_resolution: None,
        created_at_unix_secs: updated_at_unix_secs,
        updated_at_unix_secs,
    }
}

fn write_artifact(root: &std::path::Path, device_id: &str, tx_id: &str) {
    let dir = root.join(device_id).join(tx_id);
    fs::create_dir_all(&dir).expect("create artifact dir");
    fs::write(dir.join("rollback.json"), "{}").expect("write artifact");
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria-underlay-gc-{name}-{}", uuid::Uuid::new_v4()))
}
