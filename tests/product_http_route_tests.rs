use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use aria_underlay::api::operations::{
    ListOperationSummariesRequest, ListOperationSummariesResponse,
};
use aria_underlay::api::product_api::{
    HeaderProductSessionExtractor, ProductApiResponse, ProductOpsApi,
};
use aria_underlay::api::product_http::{
    ProductHttpErrorResponse, ProductHttpMethod, ProductHttpRequest, ProductHttpRouter,
    OPERATION_SUMMARIES_QUERY_PATH, PRODUCT_AUDIT_EXPORT_PATH,
    WORKER_RELOAD_STATUS_GET_PATH,
};
use aria_underlay::api::product_ops::{
    ExportProductAuditRequest, ExportProductAuditResponse,
    ProductGetWorkerReloadStatusRequest,
};
use aria_underlay::authz::RbacRole;
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, ProductAuditRecord,
    ProductAuditStore, UnderlayEvent,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::worker::daemon::{
    WorkerConfigReloadStatus, WorkerReloadCheckpoint,
};

#[test]
fn product_http_lists_operation_summaries_with_viewer_session() {
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
    let router = product_router(summary_store, Arc::new(InMemoryProductAuditStore::default()));

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: OPERATION_SUMMARIES_QUERY_PATH.into(),
        headers: product_headers("req-http-list", Some("trace-http-list"), "viewer-a", "Viewer"),
        body: json_body(&ListOperationSummariesRequest {
            attention_required_only: true,
            limit: Some(10),
            ..Default::default()
        }),
    });

    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    let body: ProductApiResponse<ListOperationSummariesResponse> = response_json(&response.body);
    assert_eq!(body.request_id, "req-http-list");
    assert_eq!(body.trace_id, "trace-http-list");
    assert_eq!(body.operator_id, "viewer-a");
    assert_eq!(body.role, RbacRole::Viewer);
    assert_eq!(body.body.overview.matched_records, 1);
    assert_eq!(body.body.summaries[0].action, "recovery.completed");
}

#[test]
fn product_http_exports_product_audit_with_auditor_session() {
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    audit_store
        .append(seed_audit_record("req-existing", "admin-a"))
        .expect("seed audit should append");
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        audit_store.clone(),
    );

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: PRODUCT_AUDIT_EXPORT_PATH.into(),
        headers: product_headers(
            "req-http-export",
            Some("trace-http-export"),
            "auditor-a",
            "Auditor",
        ),
        body: json_body(&ExportProductAuditRequest {
            reason: "quarterly audit review".into(),
            action: None,
            result: None,
            operator_id: None,
            limit: None,
        }),
    });

    assert_eq!(response.status, 200);
    let body: ProductApiResponse<ExportProductAuditResponse> = response_json(&response.body);
    assert_eq!(body.request_id, "req-http-export");
    assert_eq!(body.operator_id, "auditor-a");
    assert_eq!(body.role, RbacRole::Auditor);
    assert_eq!(body.body.overview.matched_records, 2);
    assert_eq!(
        body.body.records[1].action,
        "product_audit.export_requested"
    );
}

#[test]
fn product_http_denies_audit_export_for_operator_session() {
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: PRODUCT_AUDIT_EXPORT_PATH.into(),
        headers: product_headers(
            "req-http-denied",
            Some("trace-http-denied"),
            "operator-a",
            "Operator",
        ),
        body: json_body(&ExportProductAuditRequest {
            reason: "curious operator".into(),
            action: None,
            result: None,
            operator_id: None,
            limit: None,
        }),
    });

    assert_eq!(response.status, 403);
    let body: ProductHttpErrorResponse = response_json(&response.body);
    assert_eq!(body.request_id.as_deref(), Some("req-http-denied"));
    assert_eq!(body.trace_id.as_deref(), Some("trace-http-denied"));
    assert_eq!(body.error_code, "authorization_denied");
}

#[test]
fn product_http_viewer_can_read_worker_reload_status() {
    let temp = temp_test_dir("http-reload-status");
    let checkpoint_path = temp.join("worker-reload-checkpoint.json");
    fs::create_dir_all(&temp).expect("temp dir should be created");
    write_reload_checkpoint(&checkpoint_path, WorkerConfigReloadStatus::Rejected, 3, Some("bad interval".into()));
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: WORKER_RELOAD_STATUS_GET_PATH.into(),
        headers: product_headers("req-http-reload", Some("trace-http-reload"), "viewer-a", "Viewer"),
        body: json_body(&ProductGetWorkerReloadStatusRequest {
            checkpoint_path: checkpoint_path.clone(),
        }),
    });

    assert_eq!(response.status, 200);
    let body: ProductApiResponse<WorkerReloadCheckpoint> = response_json(&response.body);
    assert_eq!(body.request_id, "req-http-reload");
    assert_eq!(body.operator_id, "viewer-a");
    assert_eq!(body.role, RbacRole::Viewer);
    assert_eq!(body.body.status, WorkerConfigReloadStatus::Rejected);
    assert_eq!(body.body.generation, 3);
    assert_eq!(body.body.error.as_deref(), Some("bad interval"));

    fs::remove_dir_all(temp).ok();
}

#[test]
fn product_http_rejects_missing_request_id() {
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );
    let mut headers = BTreeMap::new();
    headers.insert("x-aria-operator-id".into(), "viewer-a".into());
    headers.insert("x-aria-role".into(), "Viewer".into());

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: OPERATION_SUMMARIES_QUERY_PATH.into(),
        headers,
        body: json_body(&ListOperationSummariesRequest::default()),
    });

    assert_eq!(response.status, 400);
    let body: ProductHttpErrorResponse = response_json(&response.body);
    assert_eq!(body.request_id, None);
    assert_eq!(body.error_code, "invalid_request");
}

#[test]
fn product_http_returns_404_for_unknown_path() {
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: "/product/v1/unknown".into(),
        headers: product_headers("req-http-not-found", None, "viewer-a", "Viewer"),
        body: Vec::new(),
    });

    assert_eq!(response.status, 404);
    let body: ProductHttpErrorResponse = response_json(&response.body);
    assert_eq!(body.request_id.as_deref(), Some("req-http-not-found"));
    assert_eq!(body.trace_id.as_deref(), Some("req-http-not-found"));
    assert_eq!(body.error_code, "not_found");
}

#[test]
fn product_http_returns_405_for_wrong_method_on_known_path() {
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Get,
        path: OPERATION_SUMMARIES_QUERY_PATH.into(),
        headers: product_headers("req-http-method", None, "viewer-a", "Viewer"),
        body: Vec::new(),
    });

    assert_eq!(response.status, 405);
    assert_eq!(response.headers.get("allow").map(String::as_str), Some("POST"));
    let body: ProductHttpErrorResponse = response_json(&response.body);
    assert_eq!(body.error_code, "method_not_allowed");
}

#[test]
fn product_http_rejects_malformed_json_body() {
    let router = product_router(
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: OPERATION_SUMMARIES_QUERY_PATH.into(),
        headers: product_headers("req-http-bad-json", None, "viewer-a", "Viewer"),
        body: b"{broken".to_vec(),
    });

    assert_eq!(response.status, 400);
    let body: ProductHttpErrorResponse = response_json(&response.body);
    assert_eq!(body.request_id.as_deref(), Some("req-http-bad-json"));
    assert_eq!(body.error_code, "invalid_request");
}

fn product_router(
    summary_store: Arc<InMemoryOperationSummaryStore>,
    audit_store: Arc<InMemoryProductAuditStore>,
) -> ProductHttpRouter {
    ProductHttpRouter::new(ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        summary_store,
        audit_store,
    ))
}

fn product_headers(
    request_id: &str,
    trace_id: Option<&str>,
    operator_id: &str,
    role: &str,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("x-aria-request-id".into(), request_id.into());
    if let Some(trace_id) = trace_id {
        headers.insert("x-aria-trace-id".into(), trace_id.into());
    }
    headers.insert("x-aria-operator-id".into(), operator_id.into());
    headers.insert("x-aria-role".into(), role.into());
    headers
}

fn json_body<T: serde::Serialize>(body: &T) -> Vec<u8> {
    serde_json::to_vec(body).expect("test body should serialize")
}

fn response_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> T {
    serde_json::from_slice(body).expect("response should be valid JSON")
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

fn write_reload_checkpoint(
    path: &std::path::Path,
    status: WorkerConfigReloadStatus,
    generation: u64,
    error: Option<String>,
) {
    let checkpoint = WorkerReloadCheckpoint {
        config_path: path.with_file_name("worker.json"),
        generation,
        fingerprint: format!("fingerprint-{generation}"),
        status,
        updated_at_unix_secs: 1_800_000_000,
        error,
    };
    fs::write(
        path,
        serde_json::to_vec_pretty(&checkpoint).expect("checkpoint should serialize"),
    )
    .expect("checkpoint should be written");
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-product-http-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
