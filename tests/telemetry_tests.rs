use aria_underlay::api::response::{ApplyStatus, DeviceApplyResult};
use aria_underlay::model::DeviceId;
use aria_underlay::state::drift::{DriftFinding, DriftReport, DriftType};
use aria_underlay::telemetry::{
    AuditRecord, MetricName, Metrics, UnderlayEvent, UnderlayEventKind,
};
use aria_underlay::tx::{TransactionStrategy, TxPhase};

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
