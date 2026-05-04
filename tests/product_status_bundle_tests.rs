use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::product_api::{
    HeaderProductSessionExtractor, ProductApiResponse, ProductOpsApi,
};
use aria_underlay::api::product_http::{
    ProductHttpMethod, ProductHttpRequest, ProductHttpRouter, PRODUCT_STATUS_BUNDLE_GET_PATH,
};
use aria_underlay::api::product_ops::{
    ProductStatusBundleHealthStatus, ProductStatusBundleRequest, ProductStatusBundleResponse,
};
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, JsonFileOperationAlertSink,
    OperationAlert, OperationAlertSeverity, OperationAlertSink, UnderlayEvent,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::worker::daemon::{
    WorkerConfigReloadStatus, WorkerReloadCheckpoint,
};

#[test]
fn product_status_bundle_aggregates_operations_alerts_and_reload_status() {
    let temp = temp_test_dir("bundle");
    let alert_path = temp.join("operation-alerts.jsonl");
    let reload_path = temp.join("worker-reload-checkpoint.json");
    fs::create_dir_all(&temp).expect("temp dir should be created");

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
        .expect("attention-required summary should be recorded");
    JsonFileOperationAlertSink::new(&alert_path)
        .deliver(&[critical_alert("critical-key")])
        .expect("alert should be written");
    write_reload_checkpoint(
        &reload_path,
        WorkerConfigReloadStatus::Rejected,
        7,
        Some("invalid reload candidate".into()),
    );
    let router = product_router(summary_store);

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: PRODUCT_STATUS_BUNDLE_GET_PATH.into(),
        headers: product_headers("req-bundle", Some("trace-bundle"), "viewer-a"),
        body: json_body(&ProductStatusBundleRequest {
            operation_summary: ListOperationSummariesRequest {
                attention_required_only: true,
                limit: Some(10),
                ..Default::default()
            },
            operation_alert_path: Some(alert_path.clone()),
            alert_state_path: None,
            alert_severity: None,
            alert_limit: Some(10),
            worker_reload_checkpoint_path: Some(reload_path.clone()),
        }),
    });

    assert_eq!(response.status, 200);
    let body: ProductApiResponse<ProductStatusBundleResponse> = response_json(&response.body);
    assert_eq!(body.request_id, "req-bundle");
    assert_eq!(body.operator_id, "viewer-a");
    assert_eq!(body.body.operation_summary.matched_records, 1);
    assert_eq!(body.body.operation_summary.attention_required, 1);
    assert_eq!(
        body.body
            .alert_summary
            .as_ref()
            .expect("alert summary should be present")
            .critical,
        1
    );
    assert_eq!(
        body.body
            .worker_reload
            .as_ref()
            .expect("worker reload status should be present")
            .status,
        WorkerConfigReloadStatus::Rejected
    );
    assert_eq!(
        body.body.health.status,
        ProductStatusBundleHealthStatus::AttentionRequired
    );
    assert_eq!(body.body.health.attention_required, true);

    fs::remove_dir_all(temp).ok();
}

fn product_router(summary_store: Arc<InMemoryOperationSummaryStore>) -> ProductHttpRouter {
    ProductHttpRouter::new(ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        summary_store,
        Arc::new(InMemoryProductAuditStore::default()),
    ))
}

fn product_headers(
    request_id: &str,
    trace_id: Option<&str>,
    operator_id: &str,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("x-aria-request-id".into(), request_id.into());
    if let Some(trace_id) = trace_id {
        headers.insert("x-aria-trace-id".into(), trace_id.into());
    }
    headers.insert("x-aria-operator-id".into(), operator_id.into());
    headers
}

fn critical_alert(dedupe_key: &str) -> OperationAlert {
    OperationAlert {
        dedupe_key: dedupe_key.into(),
        severity: OperationAlertSeverity::Critical,
        request_id: "req-alert".into(),
        trace_id: "trace-alert".into(),
        action: "transaction.in_doubt".into(),
        result: "in_doubt".into(),
        tx_id: Some("tx-alert".into()),
        device_id: None,
        fields: BTreeMap::new(),
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

fn json_body<T: serde::Serialize>(body: &T) -> Vec<u8> {
    serde_json::to_vec(body).expect("test body should serialize")
}

fn response_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> T {
    serde_json::from_slice(body).expect("response should be valid JSON")
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-product-status-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
