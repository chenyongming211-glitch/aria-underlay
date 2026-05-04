use std::sync::Arc;

use aria_underlay::api::force_resolve::ForceResolveTransactionRequest;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::authz::StaticAuthorizationPolicy;
use aria_underlay::device::DeviceInventory;
use aria_underlay::model::DeviceId;
use aria_underlay::telemetry::{InMemoryProductAuditStore, ProductAuditRecord, ProductAuditStore};
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use aria_underlay::{UnderlayError, UnderlayResult};

#[tokio::test]
async fn registered_operator_force_resolve_records_product_audit_before_journal_terminal() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&journal_record("tx-manual", TxPhase::InDoubt, "leaf-a"))
        .expect("in-doubt record should be stored");
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal.clone())
        .with_authorization_policy(Arc::new(
            StaticAuthorizationPolicy::new().with_operator("netops-a"),
        ))
        .with_product_audit_store(audit_store.clone());

    let response = service
        .force_resolve_transaction(force_resolve_request("tx-manual", "netops-a"))
        .await
        .expect("registered operator should be allowed to force resolve");

    assert!(response.resolved);
    assert_eq!(
        journal
            .get("tx-manual")
            .expect("journal get should succeed")
            .expect("journal record should exist")
            .phase,
        TxPhase::ForceResolved
    );

    let records = audit_store.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].action, "transaction.force_resolve_requested");
    assert_eq!(records[0].result, "authorized");
    assert_eq!(records[0].tx_id.as_deref(), Some("tx-manual"));
    assert_eq!(records[0].operator_id.as_deref(), Some("netops-a"));
    assert_eq!(
        records[0].reason.as_deref(),
        Some("validated device state out of band")
    );
}

#[tokio::test]
async fn unregistered_operator_force_resolve_is_denied_and_journal_stays_in_doubt() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&journal_record("tx-denied", TxPhase::InDoubt, "leaf-a"))
        .expect("in-doubt record should be stored");
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal.clone())
        .with_authorization_policy(Arc::new(
            StaticAuthorizationPolicy::new().with_operator("netops-a"),
        ))
        .with_product_audit_store(audit_store.clone());

    let err = service
        .force_resolve_transaction(force_resolve_request("tx-denied", "unknown-operator"))
        .await
        .expect_err("unregistered operator should not be allowed to force resolve");

    assert!(matches!(err, UnderlayError::AuthorizationDenied(_)));
    assert_eq!(
        journal
            .get("tx-denied")
            .expect("journal get should succeed")
            .expect("journal record should exist")
            .phase,
        TxPhase::InDoubt
    );
    assert!(audit_store.records().is_empty());
}

#[tokio::test]
async fn product_audit_write_failure_blocks_force_resolve_before_journal_changes() {
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(&journal_record("tx-audit-failed", TxPhase::InDoubt, "leaf-a"))
        .expect("in-doubt record should be stored");
    let service = AriaUnderlayService::new_with_journal(DeviceInventory::default(), journal.clone())
        .with_authorization_policy(Arc::new(
            StaticAuthorizationPolicy::new().with_operator("netops-a"),
        ))
        .with_product_audit_store(Arc::new(FailingProductAuditStore));

    let err = service
        .force_resolve_transaction(force_resolve_request("tx-audit-failed", "netops-a"))
        .await
        .expect_err("audit write failure should fail closed");

    assert!(matches!(err, UnderlayError::ProductAuditWriteFailed(_)));
    assert_eq!(
        journal
            .get("tx-audit-failed")
            .expect("journal get should succeed")
            .expect("journal record should exist")
            .phase,
        TxPhase::InDoubt
    );
}

#[derive(Debug)]
struct FailingProductAuditStore;

impl ProductAuditStore for FailingProductAuditStore {
    fn append(&self, _record: ProductAuditRecord) -> UnderlayResult<()> {
        Err(UnderlayError::ProductAuditWriteFailed(
            "simulated audit write failure".into(),
        ))
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        Ok(Vec::new())
    }
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

fn force_resolve_request(tx_id: &str, operator: &str) -> ForceResolveTransactionRequest {
    ForceResolveTransactionRequest {
        request_id: format!("req-resolve-{tx_id}"),
        trace_id: Some(format!("trace-resolve-{tx_id}")),
        tx_id: tx_id.into(),
        operator: operator.into(),
        reason: "validated device state out of band".into(),
        break_glass_enabled: true,
    }
}
