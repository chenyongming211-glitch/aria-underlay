use std::collections::BTreeMap;
use std::sync::Arc;

use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::product_ops::{
    ExportProductAuditRequest, ProductOperatorContext, ProductOpsManager,
};
use aria_underlay::authz::{RbacRole, StaticAuthorizationPolicy};
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, OperationSummary,
    ProductAuditRecord, ProductAuditStore, UnderlayEvent,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::{UnderlayError, UnderlayResult};

#[test]
fn viewer_with_assigned_role_can_list_operation_summaries_through_product_boundary() {
    let summary_store = Arc::new(InMemoryOperationSummaryStore::default());
    summary_store
        .record_event(&UnderlayEvent::recovery_completed(
            "req-recovery",
            "trace-recovery",
            &RecoveryReport {
                recovered: 0,
                in_doubt: 1,
                pending: 0,
                tx_ids: vec!["tx-recovery".into()],
                decisions: Vec::new(),
            },
        ))
        .expect("summary event should be recorded");
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role("viewer-a", RbacRole::Viewer)),
        summary_store,
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = manager
        .list_operation_summaries(
            context("req-list", "viewer-a"),
            ListOperationSummariesRequest {
                attention_required_only: true,
                limit: Some(10),
                ..Default::default()
            },
        )
        .expect("assigned viewer should list operation summaries");

    assert_eq!(response.overview.matched_records, 1);
    assert_eq!(response.overview.returned_records, 1);
    assert_eq!(response.overview.attention_required, 1);
    assert_eq!(response.summaries[0].action, "recovery.completed");
}

#[test]
fn unassigned_operator_cannot_list_operation_summaries() {
    let summary_store = Arc::new(InMemoryOperationSummaryStore::default());
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new()),
        summary_store,
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let err = manager
        .list_operation_summaries(
            context("req-denied", "unknown-user"),
            ListOperationSummariesRequest::default(),
        )
        .expect_err("unassigned product operator should fail closed");

    assert!(matches!(err, UnderlayError::AuthorizationDenied(_)));
}

#[test]
fn auditor_exports_product_audit_after_export_action_is_recorded() {
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    audit_store
        .append(seed_audit_record("req-existing", "admin-a"))
        .expect("seed audit record should append");
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role("auditor-a", RbacRole::Auditor)),
        Arc::new(InMemoryOperationSummaryStore::default()),
        audit_store.clone(),
    );

    let response = manager
        .export_product_audit(
            context("req-export", "auditor-a"),
            ExportProductAuditRequest {
                reason: "quarterly audit review".into(),
                action: None,
                result: None,
                operator_id: None,
                limit: None,
            },
        )
        .expect("auditor should export product audit history");

    assert_eq!(response.overview.matched_records, 2);
    assert_eq!(response.overview.returned_records, 2);
    assert_eq!(response.records[0].request_id, "req-existing");
    assert_eq!(response.records[1].request_id, "req-export");
    assert_eq!(response.records[1].action, "product_audit.export_requested");
    assert_eq!(response.records[1].operator_id.as_deref(), Some("auditor-a"));
    assert_eq!(response.records[1].role, Some(RbacRole::Auditor));

    let persisted = audit_store.list().expect("audit records should be readable");
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[1].action, "product_audit.export_requested");
}

#[test]
fn operator_cannot_export_product_audit() {
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role("operator-a", RbacRole::Operator)),
        Arc::new(InMemoryOperationSummaryStore::default()),
        audit_store.clone(),
    );

    let err = manager
        .export_product_audit(
            context("req-export-denied", "operator-a"),
            ExportProductAuditRequest {
                reason: "curious operator".into(),
                action: None,
                result: None,
                operator_id: None,
                limit: None,
            },
        )
        .expect_err("operator should not export product audit");

    assert!(matches!(err, UnderlayError::AuthorizationDenied(_)));
    assert!(audit_store.list().expect("audit list should work").is_empty());
}

#[test]
fn product_audit_write_failure_blocks_audit_export() {
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_role("admin-a", RbacRole::Admin)),
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(FailingProductAuditStore),
    );

    let err = manager
        .export_product_audit(
            context("req-export-failed", "admin-a"),
            ExportProductAuditRequest {
                reason: "incident review".into(),
                action: None,
                result: None,
                operator_id: None,
                limit: None,
            },
        )
        .expect_err("audit write failure should block export");

    assert!(matches!(err, UnderlayError::ProductAuditWriteFailed(_)));
}

#[derive(Debug)]
struct FailingProductAuditStore;

impl ProductAuditStore for FailingProductAuditStore {
    fn append(&self, _record: ProductAuditRecord) -> UnderlayResult<()> {
        Err(UnderlayError::ProductAuditWriteFailed(
            "simulated product audit write failure".into(),
        ))
    }

    fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>> {
        Ok(Vec::new())
    }
}

fn context(request_id: &str, operator: &str) -> ProductOperatorContext {
    ProductOperatorContext {
        request_id: request_id.into(),
        trace_id: Some(format!("trace-{request_id}")),
        operator: operator.into(),
    }
}

fn seed_audit_record(request_id: &str, operator: &str) -> ProductAuditRecord {
    ProductAuditRecord {
        request_id: request_id.into(),
        trace_id: format!("trace-{request_id}"),
        action: "daemon.schedule_change_requested".into(),
        result: "authorized".into(),
        tx_id: None,
        device_id: None,
        operator_id: Some(operator.into()),
        role: Some(RbacRole::Admin),
        reason: Some("seed record".into()),
        attention_required: false,
        error_code: None,
        error_message: None,
        fields: BTreeMap::new(),
        appended_at_unix_secs: 1,
    }
}
