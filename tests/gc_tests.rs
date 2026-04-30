use std::fs;

use aria_underlay::model::DeviceId;
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalRecord, TxJournalStore, TxPhase};
use aria_underlay::worker::gc::{JournalGc, RetentionPolicy};

#[test]
fn retention_policy_defaults_are_conservative() {
    let policy = RetentionPolicy::default();
    assert_eq!(policy.failed_journal_retention_days, 90);
    assert_eq!(policy.max_artifacts_per_device, 50);
}

#[tokio::test]
async fn gc_deletes_old_terminal_journal_but_keeps_in_doubt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let journal_root = temp.path().join("journal");
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
    assert!(store.get("tx-old-committed").expect("read committed").is_none());
    assert!(store.get("tx-old-in-doubt").expect("read in doubt").is_some());
}

#[tokio::test]
async fn gc_deletes_artifacts_only_for_old_terminal_transactions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let journal_root = temp.path().join("journal");
    let artifact_root = temp.path().join("artifacts");
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
    assert!(!artifact_root.join("leaf-a/tx-terminal").exists());
    assert!(artifact_root.join("leaf-a/tx-in-doubt").exists());
    assert!(store.get("tx-terminal").expect("read terminal").is_some());
}

#[tokio::test]
async fn gc_prunes_terminal_artifacts_per_device_without_touching_unknown_tx() {
    let temp = tempfile::tempdir().expect("tempdir");
    let journal_root = temp.path().join("journal");
    let artifact_root = temp.path().join("artifacts");
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
    assert!(artifact_root.join("leaf-a/tx-new").exists());
    assert!(!artifact_root.join("leaf-a/tx-old").exists());
    assert!(artifact_root.join("leaf-a/tx-unknown").exists());
}

fn journal_record(tx_id: &str, phase: TxPhase, updated_at_unix_secs: u64) -> TxJournalRecord {
    TxJournalRecord {
        tx_id: tx_id.into(),
        request_id: format!("req-{tx_id}"),
        trace_id: format!("trace-{tx_id}"),
        phase,
        devices: vec![DeviceId("leaf-a".into())],
        strategy: None,
        error_code: None,
        error_message: None,
        created_at_unix_secs: updated_at_unix_secs,
        updated_at_unix_secs,
    }
}

fn write_artifact(root: &std::path::Path, device_id: &str, tx_id: &str) {
    let dir = root.join(device_id).join(tx_id);
    fs::create_dir_all(&dir).expect("create artifact dir");
    fs::write(dir.join("rollback.json"), "{}").expect("write artifact");
}
