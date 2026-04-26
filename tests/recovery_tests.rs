use std::sync::Arc;

use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::DeviceInventory;
use aria_underlay::model::DeviceId;
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use aria_underlay::tx::recovery::RecoveryReport;

#[test]
fn recovery_report_defaults_to_zero() {
    let report = RecoveryReport::default();
    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 0);
    assert_eq!(report.pending, 0);
    assert!(report.tx_ids.is_empty());
}

#[tokio::test]
async fn recover_pending_transactions_reports_recoverable_journal_records() {
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

    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal);
    let report = service
        .recover_pending_transactions()
        .await
        .expect("recovery scan should succeed");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 1);
    assert_eq!(report.pending, 2);
    assert_eq!(report.tx_ids, vec!["tx-in-doubt", "tx-started"]);
}
