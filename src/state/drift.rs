use serde::{Deserialize, Serialize};

use crate::model::{DeviceId, InterfaceConfig, VlanConfig};
use crate::state::shadow::DeviceShadowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftPolicy {
    ReportOnly,
    BlockNewTransaction,
    AutoReconcile,
}

impl Default for DriftPolicy {
    fn default() -> Self {
        Self::ReportOnly
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftType {
    MissingVlan,
    ExtraVlan,
    VlanAttributeMismatch,
    MissingInterface,
    ExtraInterface,
    InterfaceAttributeMismatch,
    AdapterWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftFinding {
    pub drift_type: DriftType,
    pub path: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftReport {
    pub device_id: DeviceId,
    pub drift_detected: bool,
    pub findings: Vec<DriftFinding>,
    pub warnings: Vec<String>,
}

impl DriftReport {
    pub fn clean(device_id: DeviceId) -> Self {
        Self {
            device_id,
            drift_detected: false,
            findings: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn from_adapter_warnings(device_id: DeviceId, warnings: Vec<String>) -> Self {
        let findings = warnings
            .iter()
            .map(|warning| DriftFinding {
                drift_type: DriftType::AdapterWarning,
                path: "adapter.warning".into(),
                expected: None,
                actual: Some(warning.clone()),
            })
            .collect::<Vec<_>>();
        Self {
            device_id,
            drift_detected: !findings.is_empty(),
            findings,
            warnings,
        }
    }
}

pub fn detect_drift(expected: &DeviceShadowState, observed: &DeviceShadowState) -> DriftReport {
    let mut findings = Vec::new();

    for (vlan_id, expected_vlan) in &expected.vlans {
        match observed.vlans.get(vlan_id) {
            Some(observed_vlan) => {
                compare_vlan(*vlan_id, expected_vlan, observed_vlan, &mut findings)
            }
            None => findings.push(DriftFinding {
                drift_type: DriftType::MissingVlan,
                path: format!("vlans.{vlan_id}"),
                expected: Some(vlan_summary(expected_vlan)),
                actual: None,
            }),
        }
    }

    for (vlan_id, observed_vlan) in &observed.vlans {
        if !expected.vlans.contains_key(vlan_id) {
            findings.push(DriftFinding {
                drift_type: DriftType::ExtraVlan,
                path: format!("vlans.{vlan_id}"),
                expected: None,
                actual: Some(vlan_summary(observed_vlan)),
            });
        }
    }

    for (name, expected_interface) in &expected.interfaces {
        match observed.interfaces.get(name) {
            Some(observed_interface) => {
                compare_interface(name, expected_interface, observed_interface, &mut findings)
            }
            None => findings.push(DriftFinding {
                drift_type: DriftType::MissingInterface,
                path: format!("interfaces.{name}"),
                expected: Some(interface_summary(expected_interface)),
                actual: None,
            }),
        }
    }

    for (name, observed_interface) in &observed.interfaces {
        if !expected.interfaces.contains_key(name) {
            findings.push(DriftFinding {
                drift_type: DriftType::ExtraInterface,
                path: format!("interfaces.{name}"),
                expected: None,
                actual: Some(interface_summary(observed_interface)),
            });
        }
    }

    let warnings = observed.warnings.clone();
    findings.extend(warnings.iter().map(|warning| DriftFinding {
        drift_type: DriftType::AdapterWarning,
        path: "adapter.warning".into(),
        expected: None,
        actual: Some(warning.clone()),
    }));

    DriftReport {
        device_id: expected.device_id.clone(),
        drift_detected: !findings.is_empty(),
        findings,
        warnings,
    }
}

fn compare_vlan(
    vlan_id: u16,
    expected: &VlanConfig,
    observed: &VlanConfig,
    findings: &mut Vec<DriftFinding>,
) {
    if expected != observed {
        findings.push(DriftFinding {
            drift_type: DriftType::VlanAttributeMismatch,
            path: format!("vlans.{vlan_id}"),
            expected: Some(vlan_summary(expected)),
            actual: Some(vlan_summary(observed)),
        });
    }
}

fn compare_interface(
    name: &str,
    expected: &InterfaceConfig,
    observed: &InterfaceConfig,
    findings: &mut Vec<DriftFinding>,
) {
    if expected != observed {
        findings.push(DriftFinding {
            drift_type: DriftType::InterfaceAttributeMismatch,
            path: format!("interfaces.{name}"),
            expected: Some(interface_summary(expected)),
            actual: Some(interface_summary(observed)),
        });
    }
}

fn vlan_summary(vlan: &VlanConfig) -> String {
    format!(
        "id={},name={},description={}",
        vlan.vlan_id,
        vlan.name.as_deref().unwrap_or(""),
        vlan.description.as_deref().unwrap_or("")
    )
}

fn interface_summary(interface: &InterfaceConfig) -> String {
    format!(
        "name={},admin={:?},description={},mode={:?}",
        interface.name,
        interface.admin_state,
        interface.description.as_deref().unwrap_or(""),
        interface.mode
    )
}
