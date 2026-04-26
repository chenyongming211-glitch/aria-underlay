use aria_underlay::model::DeviceId;
use aria_underlay::tx::{
    choose_strategy, CapabilityFlags, EndpointLockTable, JsonFileTxJournalStore, TransactionMode,
    TransactionStrategy, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
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
fn file_journal_sanitizes_transaction_id_path() {
    let root = temp_journal_dir("sanitize");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "../bad/tx".into(),
        request_id: "req-1".into(),
        trace_id: "trace-1".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    store.put(&record).expect("journal put should succeed");

    assert!(root.join("___bad_tx.json").exists());
    assert!(store.get("../bad/tx").expect("journal get should succeed").is_some());

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

fn temp_journal_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria-underlay-journal-{name}-{}", uuid::Uuid::new_v4()))
}
