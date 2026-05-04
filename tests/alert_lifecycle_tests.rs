use std::sync::Arc;

use aria_underlay::api::alert_lifecycle::{
    AlertLifecycleManager, AlertLifecycleTransitionRequest,
};
use aria_underlay::authz::StaticAuthorizationPolicy;
use aria_underlay::telemetry::{
    InMemoryOperationAlertLifecycleStore, InMemoryProductAuditStore,
    OperationAlertLifecycleStatus, OperationAlertLifecycleStore, ProductAuditRecord,
    ProductAuditStore,
};
use aria_underlay::{UnderlayError, UnderlayResult};

#[test]
fn operator_acknowledges_alert_with_product_audit_and_history() {
    let lifecycle_store = Arc::new(InMemoryOperationAlertLifecycleStore::default());
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let manager = AlertLifecycleManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_operator("netops-a")),
        audit_store.clone(),
        lifecycle_store.clone(),
    );

    let response = manager
        .transition(transition_request(
            "critical-key",
            "netops-a",
            OperationAlertLifecycleStatus::Acknowledged,
        ))
        .expect("operator should be able to acknowledge an alert");

    assert_eq!(response.record.dedupe_key, "critical-key");
    assert_eq!(response.record.status, OperationAlertLifecycleStatus::Acknowledged);
    assert_eq!(response.record.operator_id.as_deref(), Some("netops-a"));
    assert_eq!(
        response.record.reason.as_deref(),
        Some("investigating current operation alert")
    );
    assert_eq!(response.record.history.len(), 1);
    assert_eq!(
        response.record.history[0].status,
        OperationAlertLifecycleStatus::Acknowledged
    );

    let persisted = lifecycle_store
        .get("critical-key")
        .expect("lifecycle get should succeed")
        .expect("lifecycle record should exist");
    assert_eq!(persisted.status, OperationAlertLifecycleStatus::Acknowledged);
    assert_eq!(persisted.history.len(), 1);

    let audit_records = audit_store.records();
    assert_eq!(audit_records.len(), 1);
    assert_eq!(audit_records[0].action, "alert.acknowledged");
    assert_eq!(audit_records[0].result, "authorized");
    assert_eq!(audit_records[0].operator_id.as_deref(), Some("netops-a"));
    assert_eq!(
        audit_records[0].fields.get("dedupe_key").map(String::as_str),
        Some("critical-key")
    );
    assert_eq!(
        audit_records[0].fields.get("status").map(String::as_str),
        Some("Acknowledged")
    );
}

#[test]
fn terminal_alert_lifecycle_state_rejects_later_transitions() {
    let store = InMemoryOperationAlertLifecycleStore::default();
    store
        .transition(store_transition(
            "critical-key",
            "netops-a",
            OperationAlertLifecycleStatus::Resolved,
        ))
        .expect("alert should resolve from open");

    let err = store
        .transition(store_transition(
            "critical-key",
            "netops-a",
            OperationAlertLifecycleStatus::Acknowledged,
        ))
        .expect_err("resolved alerts should be terminal");

    assert!(matches!(err, UnderlayError::InvalidIntent(_)));
    let record = store
        .get("critical-key")
        .expect("lifecycle get should succeed")
        .expect("record should exist");
    assert_eq!(record.status, OperationAlertLifecycleStatus::Resolved);
    assert_eq!(record.history.len(), 1);
}

#[test]
fn audit_write_failure_blocks_alert_lifecycle_transition() {
    let lifecycle_store = Arc::new(InMemoryOperationAlertLifecycleStore::default());
    let manager = AlertLifecycleManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_operator("netops-a")),
        Arc::new(FailingProductAuditStore),
        lifecycle_store.clone(),
    );

    let err = manager
        .transition(transition_request(
            "critical-key",
            "netops-a",
            OperationAlertLifecycleStatus::Acknowledged,
        ))
        .expect_err("audit failure should fail closed");

    assert!(matches!(err, UnderlayError::ProductAuditWriteFailed(_)));
    assert!(
        lifecycle_store
            .get("critical-key")
            .expect("lifecycle get should succeed")
            .is_none(),
        "lifecycle state should not change when product audit cannot be written"
    );
}

#[derive(Debug)]
struct FailingProductAuditStore;

impl ProductAuditStore for FailingProductAuditStore {
    fn append(&self, _record: ProductAuditRecord) -> UnderlayResult<()> {
        Err(UnderlayError::ProductAuditWriteFailed(
            "simulated product audit failure".into(),
        ))
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        Ok(Vec::new())
    }
}

fn transition_request(
    dedupe_key: &str,
    operator: &str,
    target_status: OperationAlertLifecycleStatus,
) -> AlertLifecycleTransitionRequest {
    AlertLifecycleTransitionRequest {
        request_id: format!("req-{dedupe_key}"),
        trace_id: Some(format!("trace-{dedupe_key}")),
        dedupe_key: dedupe_key.into(),
        operator: operator.into(),
        reason: "investigating current operation alert".into(),
        target_status,
    }
}

fn store_transition(
    dedupe_key: &str,
    operator: &str,
    status: OperationAlertLifecycleStatus,
) -> aria_underlay::telemetry::OperationAlertLifecycleTransition {
    aria_underlay::telemetry::OperationAlertLifecycleTransition {
        dedupe_key: dedupe_key.into(),
        status,
        operator_id: operator.into(),
        reason: Some("manual operation".into()),
        request_id: format!("req-{dedupe_key}"),
        trace_id: format!("trace-{dedupe_key}"),
    }
}
