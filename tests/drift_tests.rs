use std::collections::BTreeMap;

use aria_underlay::model::{
    AdminState, DeviceId, InterfaceConfig, PortMode, VlanConfig,
};
use aria_underlay::state::drift::{detect_drift, DriftType};
use aria_underlay::state::DeviceShadowState;
use aria_underlay::worker::drift_auditor::{DriftAuditSnapshot, DriftAuditor};

#[tokio::test]
async fn drift_auditor_initially_reports_nothing() {
    let reports = DriftAuditor::default().run_once().await;
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
    let reports = DriftAuditor::new(vec![clean, drifted]).run_once().await;

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].device_id.0, "leaf-b");
    assert_eq!(reports[0].findings[0].drift_type, DriftType::MissingVlan);
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
