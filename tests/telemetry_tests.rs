use std::sync::Arc;

use aria_underlay::api::request::DriftAuditRequest;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::api::response::{ApplyStatus, DeviceApplyResult};
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};
use aria_underlay::proto::adapter;
use aria_underlay::state::drift::{DriftFinding, DriftReport, DriftType};
use aria_underlay::telemetry::{
    AuditRecord, InMemoryEventSink, MetricName, Metrics, UnderlayEvent, UnderlayEventKind,
};
use aria_underlay::tx::{TransactionStrategy, TxPhase};

mod common;

use common::{start_test_adapter, TestAdapter};

#[test]
fn transaction_result_event_maps_committed_phase_to_completed_kind() {
    let event = UnderlayEvent::transaction_result(
        "req-1",
        "trace-1",
        "tx-1",
        Some(DeviceId("leaf-a".into())),
        TxPhase::Committed,
        Some(TransactionStrategy::ConfirmedCommit),
        "success",
    );

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionCompleted);
    assert_eq!(event.tx_id.as_deref(), Some("tx-1"));
    assert_eq!(event.device_id.as_ref().map(|id| id.0.as_str()), Some("leaf-a"));
    assert_eq!(event.result.as_deref(), Some("success"));
}

#[test]
fn transaction_result_event_maps_in_doubt_phase_to_in_doubt_kind() {
    let event = UnderlayEvent::transaction_result(
        "req-1",
        "trace-1",
        "tx-1",
        None,
        TxPhase::InDoubt,
        Some(TransactionStrategy::CandidateCommit),
        "in_doubt",
    )
    .with_error("TX_IN_DOUBT", "candidate commit result is unknown");

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionInDoubt);
    assert_eq!(event.error_code.as_deref(), Some("TX_IN_DOUBT"));
}

#[test]
fn audit_record_preserves_traceable_transaction_fields() {
    let event = UnderlayEvent::transaction_result(
        "req-1",
        "trace-1",
        "tx-1",
        Some(DeviceId("leaf-a".into())),
        TxPhase::RolledBack,
        Some(TransactionStrategy::ConfirmedCommit),
        "rolled_back",
    );

    let record = AuditRecord::from_event(&event);

    assert_eq!(record.request_id, "req-1");
    assert_eq!(record.trace_id, "trace-1");
    assert_eq!(record.tx_id.as_deref(), Some("tx-1"));
    assert_eq!(record.action, "transaction.completed");
    assert_eq!(record.result, "rolled_back");
}

#[test]
fn audit_record_maps_force_resolved_transaction_event() {
    let event = UnderlayEvent::transaction_force_resolved(
        "req-force",
        "trace-force",
        "tx-force",
        TxPhase::InDoubt,
        &[DeviceId("leaf-a".into()), DeviceId("leaf-b".into())],
        "netops-a",
        "validated device state out of band",
    );

    let record = AuditRecord::from_event(&event);

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionForceResolved);
    assert_eq!(event.phase, Some(TxPhase::ForceResolved));
    assert_eq!(event.result.as_deref(), Some("force_resolved"));
    assert_eq!(event.fields.get("operator").map(String::as_str), Some("netops-a"));
    assert_eq!(
        event.fields.get("device_count").map(String::as_str),
        Some("2")
    );
    assert_eq!(record.action, "transaction.force_resolved");
    assert_eq!(record.result, "force_resolved");
    assert_eq!(record.tx_id.as_deref(), Some("tx-force"));
}

#[test]
fn device_apply_result_maps_to_traceable_transaction_event() {
    let result = DeviceApplyResult {
        device_id: DeviceId("leaf-a".into()),
        changed: true,
        status: ApplyStatus::InDoubt,
        tx_id: Some("tx-1".into()),
        strategy: Some(TransactionStrategy::CandidateCommit),
        error_code: Some("TX_IN_DOUBT".into()),
        error_message: Some("final state is unknown".into()),
        warnings: vec!["manual recovery required".into()],
    };

    let event = UnderlayEvent::from_device_apply_result("req-1", "trace-1", &result)
        .expect("changed apply result with tx_id should produce event");

    assert_eq!(event.kind, UnderlayEventKind::UnderlayTransactionInDoubt);
    assert_eq!(event.tx_id.as_deref(), Some("tx-1"));
    assert_eq!(event.device_id.as_ref().map(|id| id.0.as_str()), Some("leaf-a"));
    assert_eq!(event.result.as_deref(), Some("in_doubt"));
    assert_eq!(event.error_code.as_deref(), Some("TX_IN_DOUBT"));
    assert_eq!(event.fields.get("warning_count").map(String::as_str), Some("1"));
}

#[test]
fn noop_apply_result_does_not_create_transaction_event() {
    let result = DeviceApplyResult {
        device_id: DeviceId("leaf-a".into()),
        changed: false,
        status: ApplyStatus::NoOpSuccess,
        tx_id: None,
        strategy: None,
        error_code: None,
        error_message: None,
        warnings: Vec::new(),
    };

    assert!(UnderlayEvent::from_device_apply_result("req-1", "trace-1", &result).is_none());
}

#[test]
fn drift_report_maps_to_drift_event() {
    let report = DriftReport {
        device_id: DeviceId("leaf-a".into()),
        drift_detected: true,
        findings: vec![DriftFinding {
            drift_type: DriftType::MissingVlan,
            path: "vlans.100".into(),
            expected: Some("id=100".into()),
            actual: None,
        }],
        warnings: Vec::new(),
    };

    let event = UnderlayEvent::drift_detected("req-1", "trace-1", &report);

    assert_eq!(event.kind, UnderlayEventKind::UnderlayDriftDetected);
    assert_eq!(event.device_id.as_ref().map(|id| id.0.as_str()), Some("leaf-a"));
    assert_eq!(event.result.as_deref(), Some("drift_detected"));
    assert_eq!(event.fields.get("finding_count").map(String::as_str), Some("1"));
    assert_eq!(event.fields.get("first_path").map(String::as_str), Some("vlans.100"));
}

#[tokio::test]
async fn service_emits_drift_event_to_configured_sink() {
    let adapter_endpoint = start_drift_adapter().await;
    let inventory = telemetry_inventory(adapter_endpoint);
    let sink = Arc::new(InMemoryEventSink::default());
    let service = AriaUnderlayService::new(inventory).with_event_sink(sink.clone());

    let response = service
        .run_drift_audit(DriftAuditRequest {
            device_ids: vec![DeviceId("leaf-a".into())],
        })
        .await
        .expect("drift audit should complete");

    assert_eq!(response.drifted_devices, vec![DeviceId("leaf-a".into())]);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayDriftDetected);
    assert_eq!(
        events[0].device_id.as_ref().map(|id| id.0.as_str()),
        Some("leaf-a")
    );
    assert_eq!(events[0].result.as_deref(), Some("drift_detected"));
}

#[test]
fn metrics_records_transaction_outcomes() {
    let mut metrics = Metrics::default();

    metrics.record_transaction_status(&ApplyStatus::Success);
    metrics.record_transaction_status(&ApplyStatus::Failed);
    metrics.record_transaction_status(&ApplyStatus::RolledBack);
    metrics.record_transaction_status(&ApplyStatus::InDoubt);

    let samples = metrics.samples();

    assert_eq!(metric_value(&samples, MetricName::TransactionTotal), 4.0);
    assert_eq!(metric_value(&samples, MetricName::TransactionFailedTotal), 1.0);
    assert_eq!(metric_value(&samples, MetricName::TransactionRollbackTotal), 1.0);
    assert_eq!(metric_value(&samples, MetricName::TransactionInDoubtTotal), 1.0);
}

fn metric_value(samples: &[aria_underlay::telemetry::MetricSample], name: MetricName) -> f64 {
    samples
        .iter()
        .find(|sample| sample.name == name)
        .map(|sample| sample.value)
        .unwrap_or_default()
}

fn telemetry_inventory(adapter_endpoint: String) -> DeviceInventory {
    let inventory = DeviceInventory::default();
    inventory
        .insert(DeviceInfo {
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            id: DeviceId("leaf-a".into()),
            management_ip: "127.0.0.1".into(),
            management_port: 830,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref: "local/leaf-a".into(),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint,
            lifecycle_state: DeviceLifecycleState::Ready,
        })
        .expect("telemetry test device should be inserted");
    inventory
}

async fn start_drift_adapter() -> String {
    start_test_adapter(TestAdapter {
        current_state: Some(adapter::ObservedDeviceState {
            device_id: "leaf-a".into(),
            vlans: Vec::new(),
            interfaces: Vec::new(),
        }),
        current_warnings: vec!["manual change detected by adapter".into()],
        ..Default::default()
    })
    .await
}
