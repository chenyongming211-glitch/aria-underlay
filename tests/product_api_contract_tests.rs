use std::collections::BTreeMap;
use std::sync::Arc;

use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::product_api::{
    HeaderProductSessionExtractor, ProductApiRequest, ProductOpsApi,
};
use aria_underlay::api::product_ops::ExportProductAuditRequest;
use aria_underlay::authz::RbacRole;
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, ProductAuditRecord,
    ProductAuditStore, UnderlayEvent,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::{UnderlayError, UnderlayResult};

#[test]
fn product_api_lists_operation_summaries_with_mock_viewer_session() {
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
    let api = ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        summary_store,
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = api
        .list_operation_summaries(ProductApiRequest {
            request_id: "req-list".into(),
            trace_id: Some("trace-list".into()),
            headers: session_headers("viewer-a", "Viewer"),
            body: ListOperationSummariesRequest {
                attention_required_only: true,
                limit: Some(10),
                ..Default::default()
            },
        })
        .expect("mock viewer session should list operation summaries");

    assert_eq!(response.request_id, "req-list");
    assert_eq!(response.trace_id, "trace-list");
    assert_eq!(response.operator_id, "viewer-a");
    assert_eq!(response.role, RbacRole::Viewer);
    assert_eq!(response.body.overview.matched_records, 1);
    assert_eq!(response.body.summaries[0].action, "recovery.completed");
}

#[test]
fn product_api_rejects_missing_operator_header() {
    let api = ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );
    let mut headers = BTreeMap::new();
    headers.insert("x-aria-role".into(), "Viewer".into());

    let err = api
        .list_operation_summaries(ProductApiRequest {
            request_id: "req-missing-operator".into(),
            trace_id: None,
            headers,
            body: ListOperationSummariesRequest::default(),
        })
        .expect_err("missing operator header should be rejected");

    assert!(matches!(err, UnderlayError::InvalidIntent(_)));
}

#[test]
fn product_api_exports_product_audit_with_mock_auditor_session() {
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    audit_store
        .append(seed_audit_record("req-existing", "admin-a"))
        .expect("seed audit should append");
    let api = ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        Arc::new(InMemoryOperationSummaryStore::default()),
        audit_store.clone(),
    );

    let response = api
        .export_product_audit(ProductApiRequest {
            request_id: "req-export".into(),
            trace_id: Some("trace-export".into()),
            headers: session_headers("auditor-a", "Auditor"),
            body: ExportProductAuditRequest {
                reason: "quarterly audit review".into(),
                action: None,
                result: None,
                operator_id: None,
                limit: None,
            },
        })
        .expect("mock auditor session should export product audit");

    assert_eq!(response.operator_id, "auditor-a");
    assert_eq!(response.role, RbacRole::Auditor);
    assert_eq!(response.body.overview.matched_records, 2);
    assert_eq!(response.body.records[0].request_id, "req-existing");
    assert_eq!(response.body.records[1].request_id, "req-export");
    assert_eq!(
        response.body.records[1].action,
        "product_audit.export_requested"
    );
}

#[test]
fn product_api_denies_audit_export_for_mock_operator_session() {
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let api = ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        Arc::new(InMemoryOperationSummaryStore::default()),
        audit_store.clone(),
    );

    let err = api
        .export_product_audit(ProductApiRequest {
            request_id: "req-denied".into(),
            trace_id: Some("trace-denied".into()),
            headers: session_headers("operator-a", "Operator"),
            body: ExportProductAuditRequest {
                reason: "curious operator".into(),
                action: None,
                result: None,
                operator_id: None,
                limit: None,
            },
        })
        .expect_err("operator should not export product audit");

    assert!(matches!(err, UnderlayError::AuthorizationDenied(_)));
    assert!(audit_store.list().expect("audit list should work").is_empty());
}

#[test]
fn product_api_audit_export_fails_closed_when_audit_append_fails() {
    let api = ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(FailingProductAuditStore),
    );

    let err = api
        .export_product_audit(ProductApiRequest {
            request_id: "req-audit-failed".into(),
            trace_id: Some("trace-audit-failed".into()),
            headers: session_headers("admin-a", "Admin"),
            body: ExportProductAuditRequest {
                reason: "incident review".into(),
                action: None,
                result: None,
                operator_id: None,
                limit: None,
            },
        })
        .expect_err("audit append failure should block export");

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

fn session_headers(operator_id: &str, role: &str) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("x-aria-operator-id".into(), operator_id.into());
    headers.insert("x-aria-role".into(), role.into());
    headers
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
