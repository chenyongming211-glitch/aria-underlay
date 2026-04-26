use std::sync::Arc;

use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};
use aria_underlay::proto::adapter;
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use aria_underlay::tx::recovery::{
    classify_recovery, in_doubt_records_for_devices, RecoveryAction, RecoveryReport,
};

mod common;

use common::{adapter_result, start_test_adapter, TestAdapter};

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

#[tokio::test]
async fn recover_pending_transactions_marks_adapter_recovery_failure_in_doubt() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-commit-lost-session".into(),
                    request_id: "req-commit-lost-session".into(),
                    trace_id: "trace-commit-lost-session".into(),
                },
                vec![DeviceId("leaf-a".into())],
            )
            .with_phase(TxPhase::Committing),
        )
        .expect("committing journal record should be stored");

    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal.clone());
    let report = service
        .recover_pending_transactions()
        .await
        .expect("recovery scan should complete with in-doubt result");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 1);
    assert_eq!(report.pending, 1);
    assert_eq!(report.decisions[0].action, RecoveryAction::AdapterRecover);

    let record = journal
        .get("tx-commit-lost-session")
        .expect("journal get should succeed")
        .expect("journal record should still exist");
    assert_eq!(record.phase, TxPhase::InDoubt);
    assert_eq!(record.error_code.as_deref(), Some("DEVICE_NOT_FOUND"));
}

#[tokio::test]
async fn recover_pending_transactions_records_adapter_rolled_back_result() {
    let endpoint = start_recovery_adapter(adapter::AdapterOperationStatus::RolledBack).await;
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-prepared".into(),
                    request_id: "req-prepared".into(),
                    trace_id: "trace-prepared".into(),
                },
                vec![DeviceId("leaf-a".into())],
            )
            .with_phase(TxPhase::Prepared),
        )
        .expect("prepared journal record should be stored");

    let service = AriaUnderlayService::new_with_journal(
        inventory_with_recovery_endpoint("leaf-a", endpoint),
        journal.clone(),
    );
    let report = service
        .recover_pending_transactions()
        .await
        .expect("adapter recovery should complete");

    assert_eq!(report.recovered, 1);
    assert_eq!(report.in_doubt, 0);
    assert_eq!(report.pending, 0);

    let record = journal
        .get("tx-prepared")
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert_eq!(record.phase, TxPhase::RolledBack);
}

#[tokio::test]
async fn recover_pending_transactions_records_adapter_in_doubt_result() {
    let endpoint = start_recovery_adapter(adapter::AdapterOperationStatus::InDoubt).await;
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-commit-unknown".into(),
                    request_id: "req-commit-unknown".into(),
                    trace_id: "trace-commit-unknown".into(),
                },
                vec![DeviceId("leaf-a".into())],
            )
            .with_phase(TxPhase::Committing),
        )
        .expect("committing journal record should be stored");

    let service = AriaUnderlayService::new_with_journal(
        inventory_with_recovery_endpoint("leaf-a", endpoint),
        journal.clone(),
    );
    let report = service
        .recover_pending_transactions()
        .await
        .expect("adapter recovery should complete");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 1);
    assert_eq!(report.pending, 1);
    assert_eq!(report.tx_ids, vec!["tx-commit-unknown"]);

    let record = journal
        .get("tx-commit-unknown")
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert_eq!(record.phase, TxPhase::InDoubt);
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

#[test]
fn in_doubt_records_for_devices_only_returns_blocking_devices() {
    let records = vec![
        journal_record("tx-leaf-a", TxPhase::InDoubt, "leaf-a"),
        journal_record("tx-leaf-b", TxPhase::Prepared, "leaf-b"),
        journal_record("tx-leaf-c", TxPhase::InDoubt, "leaf-c"),
    ];

    let blocking = in_doubt_records_for_devices(&records, &[DeviceId("leaf-a".into())]);

    assert_eq!(blocking.len(), 1);
    assert_eq!(blocking[0].tx_id, "tx-leaf-a");
}

fn journal_record(tx_id: &str, phase: TxPhase, device_id: &str) -> TxJournalRecord {
    TxJournalRecord::started(
        &TxContext {
            tx_id: tx_id.into(),
            request_id: format!("req-{tx_id}"),
            trace_id: format!("trace-{tx_id}"),
        },
        vec![DeviceId(device_id.into())],
    )
    .with_phase(phase)
}

fn inventory_with_recovery_endpoint(device_id: &str, adapter_endpoint: String) -> DeviceInventory {
    let inventory = DeviceInventory::default();
    inventory
        .insert(DeviceInfo {
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            id: DeviceId(device_id.into()),
            management_ip: "127.0.0.1".into(),
            management_port: 830,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref: format!("local/{device_id}"),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint,
            lifecycle_state: DeviceLifecycleState::Ready,
        })
        .expect("recovery device should be inserted");
    inventory
}

async fn start_recovery_adapter(status: adapter::AdapterOperationStatus) -> String {
    start_test_adapter(TestAdapter {
        recover_result: adapter_result(status),
        ..Default::default()
    })
    .await
}
