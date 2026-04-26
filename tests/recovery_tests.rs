use std::sync::Arc;

use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::DeviceInventory;
use aria_underlay::model::DeviceId;
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use aria_underlay::tx::recovery::{classify_recovery, RecoveryAction, RecoveryReport};

#[test]
fn recovery_report_defaults_to_zero() {
    let report = RecoveryReport::default();
    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 0);
    assert_eq!(report.pending, 0);
    assert!(report.tx_ids.is_empty());
    assert!(report.decisions.is_empty());
}

#[tokio::test]
async fn recover_pending_transactions_marks_unrecoverable_records_in_doubt() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&TxJournalRecord::started(
            &TxContext {
                tx_id: "tx-started".into(),
                request_id: "req-started".into(),
                trace_id: "trace-started".into(),
            },
            vec![DeviceId("leaf-a".into())],
        ))
        .expect("started journal record should be stored");
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-in-doubt".into(),
                    request_id: "req-in-doubt".into(),
                    trace_id: "trace-in-doubt".into(),
                },
                vec![DeviceId("leaf-b".into())],
            )
            .with_phase(TxPhase::InDoubt),
        )
        .expect("in-doubt journal record should be stored");
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-committed".into(),
                    request_id: "req-committed".into(),
                    trace_id: "trace-committed".into(),
                },
                vec![DeviceId("leaf-c".into())],
            )
            .with_phase(TxPhase::Committed),
        )
        .expect("committed journal record should be stored");

    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal.clone());
    let report = service
        .recover_pending_transactions()
        .await
        .expect("recovery scan should succeed");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 2);
    assert_eq!(report.pending, 2);
    assert_eq!(report.tx_ids, vec!["tx-in-doubt", "tx-started"]);
    assert_eq!(report.decisions.len(), 2);

    let started = journal
        .get("tx-started")
        .expect("journal get should succeed")
        .expect("started journal should still exist");
    assert_eq!(started.phase, TxPhase::InDoubt);
    assert_eq!(started.error_code.as_deref(), Some("DEVICE_NOT_FOUND"));
}

#[test]
fn recovery_classification_is_phase_aware() {
    let base = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-classify".into(),
            request_id: "req-classify".into(),
            trace_id: "trace-classify".into(),
        },
        vec![DeviceId("leaf-a".into())],
    );

    assert_eq!(
        classify_recovery(&base.clone().with_phase(TxPhase::Prepared)).action,
        RecoveryAction::DiscardPreparedChanges
    );
    assert_eq!(
        classify_recovery(&base.clone().with_phase(TxPhase::Committing)).action,
        RecoveryAction::AdapterRecover
    );
    assert_eq!(
        classify_recovery(&base.clone().with_phase(TxPhase::InDoubt)).action,
        RecoveryAction::ManualIntervention
    );
    assert_eq!(
        classify_recovery(&base.with_phase(TxPhase::Committed)).action,
        RecoveryAction::Noop
    );
}
