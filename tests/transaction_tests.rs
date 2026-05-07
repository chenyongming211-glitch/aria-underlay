use aria_underlay::model::DeviceId;
use aria_underlay::tx::{
    choose_strategy, CapabilityFlags, EndpointLockTable, JsonFileTxJournalStore,
    LockAcquisitionPolicy, TransactionMode, TransactionStrategy, TxContext, TxJournalRecord,
    TxJournalStore, TxPhase,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[test]
fn confirmed_commit_strategy_wins_when_supported() {
    let strategy = choose_strategy(
        CapabilityFlags {
            supports_candidate: true,
            supports_validate: true,
            supports_confirmed_commit: true,
            supports_persist_id: true,
            supports_rollback_on_error: false,
            supports_writable_running: false,
            supports_cli_fallback: false,
        },
        TransactionMode::StrictConfirmedCommit,
    );

    assert_eq!(strategy, TransactionStrategy::ConfirmedCommit);
}

#[test]
fn file_journal_round_trips_record() {
    let root = temp_journal_dir("round-trip");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-1".into(),
        request_id: "req-1".into(),
        trace_id: "trace-1".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_strategy(TransactionStrategy::ConfirmedCommit)
        .with_phase(TxPhase::Prepared);

    store.put(&record).expect("journal put should succeed");
    let loaded = store
        .get("tx-1")
        .expect("journal get should succeed")
        .expect("record should exist");

    assert_eq!(loaded.tx_id, "tx-1");
    assert_eq!(loaded.request_id, "req-1");
    assert_eq!(loaded.trace_id, "trace-1");
    assert_eq!(loaded.phase, TxPhase::Prepared);
    assert_eq!(loaded.strategy, Some(TransactionStrategy::ConfirmedCommit));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn journal_record_preserves_error_history() {
    let context = TxContext {
        tx_id: "tx-errors".into(),
        request_id: "req-errors".into(),
        trace_id: "trace-errors".into(),
    };

    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::Committing)
        .with_error("COMMIT_FAILED", "commit failed")
        .with_phase(TxPhase::InDoubt)
        .with_error("ROLLBACK_FAILED", "rollback failed");

    assert_eq!(record.error_code.as_deref(), Some("ROLLBACK_FAILED"));
    assert_eq!(record.error_history.len(), 2);
    assert_eq!(record.error_history[0].phase, TxPhase::Committing);
    assert_eq!(record.error_history[0].code, "COMMIT_FAILED");
    assert_eq!(record.error_history[1].phase, TxPhase::InDoubt);
    assert_eq!(record.error_history[1].code, "ROLLBACK_FAILED");
}

#[test]
fn file_journal_round_trips_error_history() {
    let root = temp_journal_dir("error-history");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-error-history".into(),
        request_id: "req-error-history".into(),
        trace_id: "trace-error-history".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::Committing)
        .with_error("COMMIT_FAILED", "commit failed")
        .with_phase(TxPhase::InDoubt)
        .with_error("ROLLBACK_FAILED", "rollback failed");

    store.put(&record).expect("journal put should succeed");
    let loaded = store
        .get("tx-error-history")
        .expect("journal get should succeed")
        .expect("record should exist");

    assert_eq!(loaded.error_history.len(), 2);
    assert_eq!(loaded.error_history[0].code, "COMMIT_FAILED");
    assert_eq!(loaded.error_history[1].code, "ROLLBACK_FAILED");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_round_trips_manual_resolution() {
    let root = temp_journal_dir("manual-resolution");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-manual-resolution".into(),
        request_id: "req-manual-resolution".into(),
        trace_id: "trace-manual-resolution".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::InDoubt)
        .with_manual_resolution(
            "netops-a",
            "validated device state out of band",
            "req-force",
            "trace-force",
        )
        .with_phase(TxPhase::ForceResolved);

    store.put(&record).expect("journal put should succeed");
    let loaded = store
        .get("tx-manual-resolution")
        .expect("journal get should succeed")
        .expect("record should exist");

    assert_eq!(loaded.phase, TxPhase::ForceResolved);
    let manual = loaded
        .manual_resolution
        .expect("manual resolution should round-trip through file journal");
    assert_eq!(manual.operator, "netops-a");
    assert_eq!(manual.reason, "validated device state out of band");
    assert_eq!(manual.request_id, "req-force");
    assert_eq!(manual.trace_id, "trace-force");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_lists_only_recoverable_records() {
    let root = temp_journal_dir("recoverable");
    let store = JsonFileTxJournalStore::new(&root);
    let active = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-active".into(),
            request_id: "req-active".into(),
            trace_id: "trace-active".into(),
        },
        vec![DeviceId("leaf-a".into())],
    )
    .with_phase(TxPhase::Verifying);
    let committed = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-committed".into(),
            request_id: "req-committed".into(),
            trace_id: "trace-committed".into(),
        },
        vec![DeviceId("leaf-b".into())],
    )
    .with_phase(TxPhase::Committed);

    store.put(&active).expect("active journal put should succeed");
    store
        .put(&committed)
        .expect("committed journal put should succeed");

    let recoverable = store
        .list_recoverable()
        .expect("journal list should succeed");

    assert_eq!(recoverable.len(), 1);
    assert_eq!(recoverable[0].tx_id, "tx-active");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_terminal_records_stay_non_recoverable_after_store_recreation() {
    let root = temp_journal_dir("terminal-restart");
    let store = JsonFileTxJournalStore::new(&root);
    let terminal_records = [
        ("tx-committed", TxPhase::Committed),
        ("tx-failed", TxPhase::Failed),
        ("tx-rolled-back", TxPhase::RolledBack),
        ("tx-force-resolved", TxPhase::ForceResolved),
    ];

    for (tx_id, phase) in &terminal_records {
        let record = TxJournalRecord::started(
            &TxContext {
                tx_id: (*tx_id).into(),
                request_id: format!("req-{tx_id}"),
                trace_id: format!("trace-{tx_id}"),
            },
            vec![DeviceId("leaf-a".into())],
        )
        .with_phase(phase.clone());

        store
            .put(&record)
            .expect("terminal journal put should succeed");
    }

    let restarted = JsonFileTxJournalStore::new(&root);
    let recoverable = restarted
        .list_recoverable()
        .expect("journal restart scan should succeed");

    assert!(recoverable.is_empty());
    for (tx_id, phase) in &terminal_records {
        let loaded = restarted
            .get(tx_id)
            .expect("terminal journal get should succeed")
            .expect("terminal journal should survive restart");
        assert_eq!(&loaded.phase, phase);
    }

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_rejects_corrupt_record_during_restart_scan() {
    let root = temp_journal_dir("corrupt-restart");
    std::fs::create_dir_all(&root).expect("journal root should be created");
    std::fs::write(root.join("tx-corrupt.json"), b"{not valid json")
        .expect("corrupt journal fixture should be written");

    let restarted = JsonFileTxJournalStore::new(&root);
    let err = restarted
        .list_recoverable()
        .expect_err("corrupt journal record should fail closed during recovery scan");
    let message = format!("{err}");

    assert!(
        message.contains("parse tx journal"),
        "unexpected journal parse error: {message}"
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_ignores_tmp_crash_residue_after_store_recreation() {
    let root = temp_journal_dir("tmp-residue");
    let store = JsonFileTxJournalStore::new(&root);
    let active = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-active".into(),
            request_id: "req-active".into(),
            trace_id: "trace-active".into(),
        },
        vec![DeviceId("leaf-a".into())],
    )
    .with_phase(TxPhase::Preparing);

    store.put(&active).expect("active journal put should succeed");
    std::fs::write(root.join(".tx-active.json.leftover.tmp"), b"not json")
        .expect("tmp journal residue should be written");

    let restarted = JsonFileTxJournalStore::new(&root);
    let recoverable = restarted
        .list_recoverable()
        .expect("journal restart scan should ignore tmp residue");

    assert_eq!(recoverable.len(), 1);
    assert_eq!(recoverable[0].tx_id, "tx-active");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_rejects_invalid_transaction_id_path() {
    let root = temp_journal_dir("sanitize");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "../bad/tx".into(),
        request_id: "req-1".into(),
        trace_id: "trace-1".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    let err = store
        .put(&record)
        .expect_err("invalid tx_id should be rejected instead of sanitized");

    assert!(
        format!("{err}").contains("invalid for file journal store"),
        "unexpected tx_id validation error: {err}"
    );
    assert!(!root.join("___bad_tx.json").exists());
    assert!(store
        .get("../bad/tx")
        .expect_err("invalid get should fail")
        .to_string()
        .contains("invalid"));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_serializes_concurrent_same_transaction_writes() {
    let root = temp_journal_dir("concurrent");
    let store = Arc::new(JsonFileTxJournalStore::new(&root));

    let writers = (0..24)
        .map(|index| {
            let store = store.clone();
            std::thread::spawn(move || {
                let context = TxContext {
                    tx_id: "tx-concurrent".into(),
                    request_id: format!("req-{index}"),
                    trace_id: format!("trace-{index}"),
                };
                let phase = if index % 2 == 0 {
                    TxPhase::Preparing
                } else {
                    TxPhase::Verifying
                };
                let record =
                    TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
                        .with_phase(phase);

                store
                    .put(&record)
                    .expect("concurrent file journal put should succeed");
            })
        })
        .collect::<Vec<_>>();

    for writer in writers {
        writer
            .join()
            .expect("journal writer thread should not panic");
    }

    let loaded = store
        .get("tx-concurrent")
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert!(loaded.request_id.starts_with("req-"));
    assert!(
        std::fs::read_dir(&root)
            .expect("journal root should be readable")
            .all(|entry| !entry
                .expect("journal entry should be readable")
                .path()
                .to_string_lossy()
                .ends_with(".tmp"))
    );

    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn endpoint_lock_serializes_same_endpoint_writers() {
    let locks = EndpointLockTable::default();
    let first_guard = locks
        .acquire_many(&[DeviceId("leaf-a".into())])
        .await
        .expect("first lock should be acquired");
    let acquired = Arc::new(AtomicBool::new(false));
    let second_acquired = acquired.clone();
    let second_locks = locks.clone();

    let second = tokio::spawn(async move {
        let _guard = second_locks
            .acquire_many(&[DeviceId("leaf-a".into())])
            .await
            .expect("second lock should eventually be acquired");
        second_acquired.store(true, Ordering::SeqCst);
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!acquired.load(Ordering::SeqCst));

    drop(first_guard);
    second.await.expect("second lock task should finish");
    assert!(acquired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn endpoint_lock_orders_multiple_endpoints_without_deadlock() {
    let locks = EndpointLockTable::default();
    let first_locks = locks.clone();
    let second_locks = locks.clone();

    let first = tokio::spawn(async move {
        let _guard = first_locks
            .acquire_many(&[DeviceId("leaf-b".into()), DeviceId("leaf-a".into())])
            .await
            .expect("first multi endpoint lock should be acquired");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    });
    let second = tokio::spawn(async move {
        let _guard = second_locks
            .acquire_many(&[DeviceId("leaf-a".into()), DeviceId("leaf-b".into())])
            .await
            .expect("second multi endpoint lock should be acquired");
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        first.await.expect("first lock task should finish");
        second.await.expect("second lock task should finish");
    })
    .await
    .expect("ordered endpoint locking should not deadlock");
}

#[tokio::test]
async fn endpoint_lock_policy_times_out_instead_of_waiting_forever() {
    let locks = EndpointLockTable::default();
    let _first_guard = locks
        .acquire_many(&[DeviceId("leaf-a".into())])
        .await
        .expect("first lock should be acquired");
    let policy = LockAcquisitionPolicy {
        max_wait_secs: 0,
        initial_delay_ms: 1,
        max_delay_secs: 1,
        jitter: false,
        force_unlock_enabled: false,
    };

    let err = locks
        .acquire_many_with_policy(&[DeviceId("leaf-a".into())], &policy)
        .await
        .expect_err("second lock should time out");

    assert!(format!("{err}").contains("ENDPOINT_LOCK_TIMEOUT"));
}

fn temp_journal_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria-underlay-journal-{name}-{}", uuid::Uuid::new_v4()))
}
