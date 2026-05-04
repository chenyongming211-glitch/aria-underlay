use std::fs;
use std::collections::BTreeMap;
use std::sync::Arc;

use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::product_ops::{
    ExportProductAuditRequest, ProductGetWorkerReloadStatusRequest,
    ProductOperatorContext, ProductOpsManager,
};
use aria_underlay::authz::StaticAuthorizationPolicy;
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, OperationSummary,
    ProductAuditRecord, ProductAuditStore, UnderlayEvent,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::worker::daemon::{
    WorkerConfigReloadStatus, WorkerReloadCheckpoint,
};
use aria_underlay::{UnderlayError, UnderlayResult};

#[test]
fn registered_operator_can_list_operation_summaries_through_product_boundary() {
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
        Arc::new(StaticAuthorizationPolicy::new().with_operator("viewer-a")),
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
        .expect("registered operator should list operation summaries");

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
fn registered_operator_exports_product_audit_after_export_action_is_recorded() {
    let audit_store = Arc::new(InMemoryProductAuditStore::default());
    audit_store
        .append(seed_audit_record("req-existing", "admin-a"))
        .expect("seed audit record should append");
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_operator("auditor-a")),
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
        .expect("registered operator should export product audit history");

    assert_eq!(response.overview.matched_records, 2);
    assert_eq!(response.overview.returned_records, 2);
    assert_eq!(response.records[0].request_id, "req-existing");
    assert_eq!(response.records[1].request_id, "req-export");
    assert_eq!(response.records[1].action, "product_audit.export_requested");
    assert_eq!(response.records[1].operator_id.as_deref(), Some("auditor-a"));

    let persisted = audit_store.list().expect("audit records should be readable");
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[1].action, "product_audit.export_requested");
}

#[test]
fn product_audit_write_failure_blocks_audit_export() {
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_operator("admin-a")),
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

#[test]
fn registered_operator_can_read_worker_reload_status_through_product_boundary() {
    let temp = temp_test_dir("product-reload-status");
    let checkpoint_path = temp.join("worker-reload-checkpoint.json");
    fs::create_dir_all(&temp).expect("temp dir should be created");
    write_reload_checkpoint(&checkpoint_path, WorkerConfigReloadStatus::Applied, 4, None);
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new().with_operator("viewer-a")),
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let checkpoint = manager
        .get_worker_reload_status(
            context("req-reload-status", "viewer-a"),
            ProductGetWorkerReloadStatusRequest {
                checkpoint_path: checkpoint_path.clone(),
            },
        )
        .expect("registered operator should read worker reload status");

    assert_eq!(checkpoint.status, WorkerConfigReloadStatus::Applied);
    assert_eq!(checkpoint.generation, 4);
    assert_eq!(checkpoint.error, None);

    fs::remove_dir_all(temp).ok();
}

#[test]
fn unassigned_operator_cannot_read_worker_reload_status() {
    let temp = temp_test_dir("product-reload-status-denied");
    let checkpoint_path = temp.join("worker-reload-checkpoint.json");
    fs::create_dir_all(&temp).expect("temp dir should be created");
    write_reload_checkpoint(&checkpoint_path, WorkerConfigReloadStatus::Started, 1, None);
    let manager = ProductOpsManager::new(
        Arc::new(StaticAuthorizationPolicy::new()),
        Arc::new(InMemoryOperationSummaryStore::default()),
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let err = manager
        .get_worker_reload_status(
            context("req-reload-status-denied", "unknown-user"),
            ProductGetWorkerReloadStatusRequest {
                checkpoint_path: checkpoint_path.clone(),
            },
        )
        .expect_err("unassigned product operator should fail closed");

    assert!(matches!(err, UnderlayError::AuthorizationDenied(_)));

    fs::remove_dir_all(temp).ok();
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
        "aria-underlay-product-ops-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}
