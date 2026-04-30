use std::sync::Arc;
use std::sync::Mutex;

use aria_underlay::api::force_resolve::ForceResolveTransactionRequest;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};
use aria_underlay::proto::adapter;
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use aria_underlay::UnderlayError;
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

#[tokio::test]
async fn recover_pending_transactions_reloads_candidate_before_recovery() {
    let stale_record = journal_record("tx-stale", TxPhase::Prepared, "leaf-a");
    let committed_record = stale_record.clone().with_phase(TxPhase::Committed);
    let journal = Arc::new(StaleListJournalStore::new(
        vec![stale_record],
        committed_record,
    ));
    let service = AriaUnderlayService::new_with_journal(
        inventory_with_recovery_endpoint("leaf-a", "http://127.0.0.1:59999".into()),
        journal.clone(),
    );

    let report = service
        .recover_pending_transactions()
        .await
        .expect("stale recovery scan should be revalidated under lock");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.in_doubt, 0);
    assert_eq!(report.pending, 0);
    let record = journal
        .get("tx-stale")
        .expect("journal get should succeed")
        .expect("journal record should still exist");
    assert_eq!(record.phase, TxPhase::Committed);
}

#[tokio::test]
async fn force_resolve_transaction_marks_in_doubt_record_terminal() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&journal_record("tx-manual", TxPhase::InDoubt, "leaf-a"))
        .expect("in-doubt journal record should be stored");
    let service = AriaUnderlayService::new_with_journal(
        inventory_with_recovery_endpoint("leaf-a", "http://127.0.0.1:59999".into()),
        journal.clone(),
    );

    let response = service
        .force_resolve_transaction(force_resolve_request("tx-manual"))
        .await
        .expect("break-glass force resolve should succeed for in-doubt transaction");

    assert!(response.resolved);
    assert_eq!(response.tx_id, "tx-manual");
    assert_eq!(response.previous_phase, TxPhase::InDoubt);
    assert_eq!(response.resolved_phase, TxPhase::ForceResolved);
    assert_eq!(response.devices, vec![DeviceId("leaf-a".into())]);

    let record = journal
        .get("tx-manual")
        .expect("journal get should succeed")
        .expect("journal record should still exist");
    assert_eq!(record.phase, TxPhase::ForceResolved);
    let manual = record
        .manual_resolution
        .expect("force-resolved record should include manual resolution metadata");
    assert_eq!(manual.operator, "netops-a");
    assert_eq!(manual.reason, "validated device state out of band");
    assert!(
        journal
            .list_recoverable()
            .expect("journal list should succeed")
            .is_empty(),
        "force-resolved transaction must no longer block new transactions"
    );
}

#[tokio::test]
async fn force_resolve_transaction_requires_break_glass() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&journal_record("tx-no-break-glass", TxPhase::InDoubt, "leaf-a"))
        .expect("in-doubt journal record should be stored");
    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal.clone());
    let mut request = force_resolve_request("tx-no-break-glass");
    request.break_glass_enabled = false;

    let err = service
        .force_resolve_transaction(request)
        .await
        .expect_err("force resolve must require explicit break-glass flag");

    match err {
        UnderlayError::AdapterOperation { code, message, .. } => {
            assert_eq!(code, "FORCE_RESOLVE_BREAK_GLASS_REQUIRED");
            assert_eq!(
                message,
                "break-glass must be enabled to force resolve an in-doubt transaction"
            );
        }
        other => panic!("unexpected error: {other}"),
    }
    let record = journal
        .get("tx-no-break-glass")
        .expect("journal get should succeed")
        .expect("journal record should still exist");
    assert_eq!(record.phase, TxPhase::InDoubt);
}

#[tokio::test]
async fn force_resolve_transaction_rejects_non_in_doubt_record() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&journal_record("tx-committed", TxPhase::Committed, "leaf-a"))
        .expect("committed journal record should be stored");
    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal);

    let err = service
        .force_resolve_transaction(force_resolve_request("tx-committed"))
        .await
        .expect_err("force resolve should only apply to in-doubt transactions");

    match err {
        UnderlayError::AdapterOperation { code, message, .. } => {
            assert_eq!(code, "TX_NOT_IN_DOUBT");
            assert_eq!(message, "transaction tx-committed is Committed, not InDoubt");
        }
        other => panic!("unexpected error: {other}"),
    }
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
        classify_recovery(&base.clone().with_phase(TxPhase::Committed)).action,
        RecoveryAction::Noop
    );
    assert_eq!(
        classify_recovery(&base.with_phase(TxPhase::ForceResolved)).action,
        RecoveryAction::Noop
    );
}

#[derive(Debug)]
struct StaleListJournalStore {
    stale_records: Mutex<Vec<TxJournalRecord>>,
    current_record: Mutex<TxJournalRecord>,
}

impl StaleListJournalStore {
    fn new(stale_records: Vec<TxJournalRecord>, current_record: TxJournalRecord) -> Self {
        Self {
            stale_records: Mutex::new(stale_records),
            current_record: Mutex::new(current_record),
        }
    }
}

impl TxJournalStore for StaleListJournalStore {
    fn put(&self, record: &TxJournalRecord) -> aria_underlay::UnderlayResult<()> {
        *self.current_record.lock().map_err(|_| {
            aria_underlay::UnderlayError::Internal("journal mutex poisoned".into())
        })? = record.clone();
        Ok(())
    }

    fn get(&self, tx_id: &str) -> aria_underlay::UnderlayResult<Option<TxJournalRecord>> {
        let record = self
            .current_record
            .lock()
            .map_err(|_| {
                aria_underlay::UnderlayError::Internal("journal mutex poisoned".into())
            })?;
        Ok((record.tx_id == tx_id).then(|| record.clone()))
    }

    fn list_recoverable(&self) -> aria_underlay::UnderlayResult<Vec<TxJournalRecord>> {
        let mut stale = self
            .stale_records
            .lock()
            .map_err(|_| {
                aria_underlay::UnderlayError::Internal("journal mutex poisoned".into())
            })?;
        if stale.is_empty() {
            let current = self.current_record.lock().map_err(|_| {
                aria_underlay::UnderlayError::Internal("journal mutex poisoned".into())
            })?;
            Ok(current
                .phase
                .requires_recovery()
                .then(|| current.clone())
                .into_iter()
                .collect())
        } else {
            Ok(std::mem::take(&mut *stale))
        }
    }
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

fn force_resolve_request(tx_id: &str) -> ForceResolveTransactionRequest {
    ForceResolveTransactionRequest {
        request_id: format!("req-resolve-{tx_id}"),
        trace_id: Some(format!("trace-resolve-{tx_id}")),
        tx_id: tx_id.into(),
        operator: "netops-a".into(),
        reason: "validated device state out of band".into(),
        break_glass_enabled: true,
    }
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
