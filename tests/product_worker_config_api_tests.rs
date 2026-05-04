use std::fs;
use std::sync::Arc;

use aria_underlay::api::product_api::{
    HeaderProductSessionExtractor, ProductApiResponse, ProductOpsApi,
};
use aria_underlay::api::product_http::{
    ProductHttpMethod, ProductHttpRequest, ProductHttpRouter, WORKER_CONFIG_SCHEDULE_CHANGE_PATH,
};
use aria_underlay::api::product_ops::ProductChangeWorkerScheduleRequest;
use aria_underlay::api::worker_config_admin::{
    WorkerConfigAdminResponse, WorkerScheduleTarget,
};
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, OperationSummaryRetentionPolicy,
};
use aria_underlay::worker::daemon::{
    DriftAuditDaemonConfig, JournalGcDaemonConfig, OperationAlertDaemonConfig,
    OperationSummaryDaemonConfig, UnderlayWorkerDaemonConfig, WorkerScheduleConfig,
};
use aria_underlay::worker::gc::RetentionPolicy;

#[test]
fn product_http_operator_changes_worker_schedule_with_product_audit() {
    let temp = temp_test_dir("schedule-admin");
    let config_path = temp.join("worker.json");
    worker_config(&temp)
        .write_to_path(&config_path)
        .expect("worker config should be written");
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    let router = product_router(audit_store.clone());

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: WORKER_CONFIG_SCHEDULE_CHANGE_PATH.into(),
        headers: product_headers(
            "req-product-schedule",
            Some("trace-product-schedule"),
            "netops-a",
        ),
        body: json_body(&ProductChangeWorkerScheduleRequest {
            config_path: config_path.clone(),
            reason: "slow drift audit during maintenance".into(),
            target: WorkerScheduleTarget::DriftAudit,
            schedule: WorkerScheduleConfig {
                interval_secs: 900,
                run_immediately: false,
            },
        }),
    });

    assert_eq!(response.status, 200);
    let body: ProductApiResponse<WorkerConfigAdminResponse> = response_json(&response.body);
    assert_eq!(body.operator_id, "netops-a");
    assert_eq!(body.body.target, "drift_audit");
    let updated = UnderlayWorkerDaemonConfig::from_path(&config_path)
        .expect("updated config should parse");
    assert_eq!(
        updated
            .drift_audit
            .expect("drift audit should exist")
            .schedule
            .interval_secs,
        900
    );
    let records = audit_store.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].action, "daemon.schedule_change_requested");
    assert_eq!(records[0].operator_id.as_deref(), Some("netops-a"));

    fs::remove_dir_all(temp).ok();
}

fn product_router(audit_store: Arc<InMemoryProductAuditStore>) -> ProductHttpRouter {
    ProductHttpRouter::new(ProductOpsApi::new(
        Arc::new(HeaderProductSessionExtractor::default()),
        Arc::new(InMemoryOperationSummaryStore::default()),
        audit_store,
    ))
}

fn product_headers(
    request_id: &str,
    trace_id: Option<&str>,
    operator_id: &str,
) -> std::collections::BTreeMap<String, String> {
    let mut headers = std::collections::BTreeMap::new();
    headers.insert("x-aria-request-id".into(), request_id.into());
    if let Some(trace_id) = trace_id {
        headers.insert("x-aria-trace-id".into(), trace_id.into());
    }
    headers.insert("x-aria-operator-id".into(), operator_id.into());
    headers
}

fn json_body<T: serde::Serialize>(body: &T) -> Vec<u8> {
    serde_json::to_vec(body).expect("test body should serialize")
}

fn response_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> T {
    serde_json::from_slice(body).expect("response should be valid JSON")
}

fn worker_config(temp: &std::path::Path) -> UnderlayWorkerDaemonConfig {
    UnderlayWorkerDaemonConfig {
        reload: None,
        operation_summary: Some(OperationSummaryDaemonConfig {
            path: temp.join("ops").join("summaries.jsonl"),
            retention: OperationSummaryRetentionPolicy::default(),
            retention_schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
        operation_audit: None,
        operation_alert: Some(OperationAlertDaemonConfig {
            path: temp.join("ops").join("alerts.jsonl"),
            checkpoint_path: temp.join("ops").join("alert-checkpoint.json"),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
        journal_gc: Some(JournalGcDaemonConfig {
            journal_root: temp.join("journal"),
            artifact_root: Some(temp.join("artifacts")),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
            retention: RetentionPolicy::default(),
        }),
        drift_audit: Some(DriftAuditDaemonConfig {
            expected_shadow_root: temp.join("expected-shadow"),
            observed_shadow_root: temp.join("observed-shadow"),
            schedule: WorkerScheduleConfig {
                interval_secs: 60,
                run_immediately: true,
            },
        }),
    }
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-product-worker-config-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
