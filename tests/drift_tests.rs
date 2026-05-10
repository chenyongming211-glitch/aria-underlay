use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use aria_underlay::model::{
    AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig,
};
use aria_underlay::state::drift::{detect_drift, DriftType};
use aria_underlay::state::{DeviceShadowState, InMemoryShadowStateStore, ShadowStateStore};
use aria_underlay::telemetry::{InMemoryEventSink, UnderlayEventKind};
use aria_underlay::worker::drift_auditor::{
    DriftAuditSchedule, DriftAuditSnapshot, DriftAuditWorker, DriftAuditor,
    DriftObservationSource,
};
use aria_underlay::{UnderlayError, UnderlayResult};

#[tokio::test]
async fn drift_auditor_initially_reports_nothing() {
    let reports = DriftAuditor::default()
        .run_once()
        .await
        .expect("empty drift audit should succeed");
    assert!(reports.is_empty());
}

#[test]
fn detects_extra_vlan_as_drift() {
    let expected = shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]);
    let observed = shadow_state(
        "leaf-a",
        vec![vlan(100, "prod"), vlan(200, "manual")],
        vec![],
    );

    let report = detect_drift(&expected, &observed);

    assert!(report.drift_detected);
    assert_eq!(report.findings[0].drift_type, DriftType::ExtraVlan);
    assert_eq!(report.findings[0].path, "vlans.200");
}

#[test]
fn detects_interface_attribute_mismatch_as_drift() {
    let expected = shadow_state("leaf-a", vec![], vec![access_interface("GE1/0/1", 100)]);
    let observed = shadow_state("leaf-a", vec![], vec![access_interface("GE1/0/1", 200)]);

    let report = detect_drift(&expected, &observed);

    assert!(report.drift_detected);
    assert_eq!(
        report.findings[0].drift_type,
        DriftType::InterfaceAttributeMismatch
    );
    assert_eq!(report.findings[0].path, "interfaces.GE1/0/1");
}

#[test]
fn normalized_equivalent_state_is_not_drift() {
    let expected = DeviceShadowState {
        device_id: DeviceId("leaf-a".into()),
        revision: 1,
        vlans: BTreeMap::from([(
            100,
            VlanConfig {
                vlan_id: 100,
                name: Some("".into()),
                description: Some("".into()),
            },
        )]),
        interfaces: BTreeMap::from([(
            "GigabitEthernet1/0/1".into(),
            InterfaceConfig {
                name: "GigabitEthernet1/0/1".into(),
                admin_state: AdminState::Up,
                description: Some("".into()),
                mode: PortMode::Trunk {
                    native_vlan: None,
                    allowed_vlans: vec![200, 100, 100],
                },
            },
        )]),
        acls: BTreeMap::new(),
        acl_bindings: BTreeMap::new(),
        warnings: Vec::new(),
    };
    let observed = DeviceShadowState {
        device_id: DeviceId("leaf-a".into()),
        revision: 7,
        vlans: BTreeMap::from([(
            100,
            VlanConfig {
                vlan_id: 100,
                name: None,
                description: None,
            },
        )]),
        interfaces: BTreeMap::from([(
            "GE1/0/1".into(),
            InterfaceConfig {
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Trunk {
                    native_vlan: None,
                    allowed_vlans: vec![100, 200],
                },
            },
        )]),
        acls: BTreeMap::new(),
        acl_bindings: BTreeMap::new(),
        warnings: Vec::new(),
    };

    let report = detect_drift(&expected, &observed);

    assert!(!report.drift_detected, "unexpected drift findings: {:?}", report.findings);
    assert!(report.findings.is_empty());
}

#[tokio::test]
async fn drift_auditor_reports_only_drifted_snapshots() {
    let clean = DriftAuditSnapshot {
        expected: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
    };
    let drifted = DriftAuditSnapshot {
        expected: shadow_state("leaf-b", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-b", vec![], vec![]),
    };
    let reports = DriftAuditor::new(vec![clean, drifted])
        .run_once()
        .await
        .expect("snapshot drift audit should succeed");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].device_id.0, "leaf-b");
    assert_eq!(reports[0].findings[0].drift_type, DriftType::MissingVlan);
}

#[tokio::test]
async fn drift_auditor_can_compare_shadow_store_with_observed_source() {
    let expected_store = Arc::new(InMemoryShadowStateStore::default());
    expected_store
        .put(shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]))
        .expect("shadow state should be stored");

    let observed_source = Arc::new(StaticObservationSource {
        states: BTreeMap::from([(
            DeviceId("leaf-a".into()),
            shadow_state("leaf-a", vec![vlan(200, "manual")], vec![]),
        )]),
    });
    let auditor = DriftAuditor::from_source(expected_store, observed_source);

    let reports = auditor
        .run_once()
        .await
        .expect("drift audit should succeed");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].device_id.0, "leaf-a");
    assert_eq!(reports[0].findings[0].drift_type, DriftType::MissingVlan);
}

#[tokio::test]
async fn drift_auditor_summary_counts_clean_and_drifted_devices() {
    let clean = DriftAuditSnapshot {
        expected: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
    };
    let drifted = DriftAuditSnapshot {
        expected: shadow_state("leaf-b", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-b", vec![], vec![]),
    };

    let summary = DriftAuditor::new(vec![clean, drifted])
        .run_once_with_summary()
        .await
        .expect("drift summary should succeed");

    assert_eq!(summary.audited_devices, 2);
    assert_eq!(summary.drifted_devices, vec![DeviceId("leaf-b".into())]);
    assert_eq!(summary.reports.len(), 1);
}

#[tokio::test]
async fn drift_auditor_reports_expected_store_listing_failure_without_failing_run() {
    let auditor = DriftAuditor::from_source(
        Arc::new(FailingExpectedShadowStateStore),
        Arc::new(StaticObservationSource {
            states: BTreeMap::new(),
        }),
    );

    let summary = auditor
        .run_once_with_summary()
        .await
        .expect("expected-store listing failure should be reported without failing audit run");

    assert_eq!(summary.audited_devices, 0);
    assert!(summary.failed_devices.is_empty());
    assert!(summary.drifted_devices.is_empty());
    assert!(summary.reports.is_empty());
    assert_eq!(
        summary.expected_store_listing_error.as_deref(),
        Some("internal error: expected store list failed")
    );
}

#[tokio::test]
async fn drift_audit_worker_emits_detected_and_completed_events() {
    let sink = Arc::new(InMemoryEventSink::default());
    let clean = DriftAuditSnapshot {
        expected: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
    };
    let drifted = DriftAuditSnapshot {
        expected: shadow_state("leaf-b", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-b", vec![], vec![]),
    };
    let worker = DriftAuditWorker::new(DriftAuditor::new(vec![clean, drifted]), sink.clone())
        .with_request_context("req-drift", "trace-drift");

    let summary = worker
        .run_once_and_emit()
        .await
        .expect("drift worker should run once");

    assert_eq!(summary.audited_devices, 2);
    assert_eq!(summary.drifted_devices, vec![DeviceId("leaf-b".into())]);
    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayDriftDetected);
    assert_eq!(events[0].request_id, "req-drift");
    assert_eq!(events[0].device_id, Some(DeviceId("leaf-b".into())));
    assert_eq!(events[1].kind, UnderlayEventKind::UnderlayDriftAuditCompleted);
    assert_eq!(
        events[1].fields.get("audited_device_count").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        events[1].fields.get("drifted_device_count").map(String::as_str),
        Some("1")
    );
}

#[tokio::test]
async fn drift_audit_worker_emits_completed_event_for_expected_store_listing_failure() {
    let sink = Arc::new(InMemoryEventSink::default());
    let auditor = DriftAuditor::from_source(
        Arc::new(FailingExpectedShadowStateStore),
        Arc::new(StaticObservationSource {
            states: BTreeMap::new(),
        }),
    );
    let worker = DriftAuditWorker::new(auditor, sink.clone())
        .with_request_context("req-drift", "trace-drift");

    let summary = worker
        .run_once_and_emit()
        .await
        .expect("drift worker should emit a completion event for expected-store listing failure");

    assert_eq!(
        summary.expected_store_listing_error.as_deref(),
        Some("internal error: expected store list failed")
    );
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, UnderlayEventKind::UnderlayDriftAuditCompleted);
    assert_eq!(events[0].request_id, "req-drift");
    assert_eq!(events[0].result.as_deref(), Some("partial_failure"));
    assert_eq!(
        events[0].error_code.as_deref(),
        Some("DRIFT_EXPECTED_STORE_LIST_FAILED")
    );
    assert_eq!(
        events[0]
            .fields
            .get("expected_store_listing_failed")
            .map(String::as_str),
        Some("true")
    );
    assert_eq!(
        events[0].fields.get("audited_device_count").map(String::as_str),
        Some("0")
    );
}

#[tokio::test]
async fn drift_audit_worker_periodic_runner_runs_immediate_cycle_and_stops_on_shutdown() {
    let sink = Arc::new(InMemoryEventSink::default());
    let drifted = DriftAuditSnapshot {
        expected: shadow_state("leaf-a", vec![vlan(100, "prod")], vec![]),
        observed: shadow_state("leaf-a", vec![], vec![]),
    };
    let worker = DriftAuditWorker::new(DriftAuditor::new(vec![drifted]), sink.clone());

    let report = worker
        .run_periodic_until_shutdown(
            DriftAuditSchedule {
                interval_secs: 60 * 60,
                run_immediately: true,
            },
            async {},
        )
        .await
        .expect("periodic drift worker should stop cleanly on shutdown");

    assert_eq!(report.runs, 1);
    assert_eq!(
        report
            .last_summary
            .expect("periodic worker should retain last summary")
            .drifted_devices,
        vec![DeviceId("leaf-a".into())]
    );
    assert_eq!(sink.events().len(), 2);
}

#[tokio::test]
async fn drift_audit_worker_rejects_zero_second_periodic_interval() {
    let sink = Arc::new(InMemoryEventSink::default());
    let worker = DriftAuditWorker::new(DriftAuditor::default(), sink);

    let err = worker
        .run_periodic_until_shutdown(
            DriftAuditSchedule {
                interval_secs: 0,
                run_immediately: false,
            },
            async {},
        )
        .await
        .expect_err("zero interval should fail closed");
    let message = format!("{err}");

    assert!(
        message.contains("interval_secs"),
        "unexpected drift interval validation error: {message}"
    );
}

#[derive(Debug)]
struct StaticObservationSource {
    states: BTreeMap<DeviceId, DeviceShadowState>,
}

#[async_trait]
impl DriftObservationSource for StaticObservationSource {
    async fn get_observed_state(&self, device_id: &DeviceId) -> UnderlayResult<DeviceShadowState> {
        self.states
            .get(device_id)
            .cloned()
            .ok_or_else(|| aria_underlay::UnderlayError::DeviceNotFound(device_id.0.clone()))
    }
}

#[derive(Debug)]
struct FailingExpectedShadowStateStore;

impl ShadowStateStore for FailingExpectedShadowStateStore {
    fn get(&self, _device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        Ok(None)
    }

    fn put(&self, state: DeviceShadowState) -> UnderlayResult<DeviceShadowState> {
        Ok(state)
    }

    fn remove(&self, _device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        Ok(None)
    }

    fn list(&self) -> UnderlayResult<Vec<DeviceShadowState>> {
        Err(UnderlayError::Internal("expected store list failed".into()))
    }
}

fn shadow_state(
    device_id: &str,
    vlans: Vec<VlanConfig>,
    interfaces: Vec<InterfaceConfig>,
) -> DeviceShadowState {
    DeviceShadowState {
        device_id: DeviceId(device_id.into()),
        revision: 1,
        vlans: vlans
            .into_iter()
            .map(|vlan| (vlan.vlan_id, vlan))
            .collect::<BTreeMap<_, _>>(),
        interfaces: interfaces
            .into_iter()
            .map(|interface| (interface.name.clone(), interface))
            .collect::<BTreeMap<_, _>>(),
        acls: BTreeMap::new(),
        acl_bindings: BTreeMap::new(),
        warnings: Vec::new(),
    }
}

fn vlan(vlan_id: u16, name: &str) -> VlanConfig {
    VlanConfig {
        vlan_id,
        name: Some(name.into()),
        description: None,
    }
}

fn access_interface(name: &str, vlan_id: u16) -> InterfaceConfig {
    InterfaceConfig {
        name: name.into(),
        admin_state: AdminState::Up,
        description: None,
        mode: PortMode::Access { vlan_id },
    }
}
