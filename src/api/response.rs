use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::device::DeviceLifecycleState;
use crate::engine::change_plan::ChangePlan;
use crate::engine::diff::{ChangeOp, ChangeSet};
use crate::model::{acl_binding_key, DeviceId};
use crate::tx::TransactionStrategy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyStatus {
    NoOpSuccess,
    Success,
    SuccessWithWarning,
    PartialSuccess,
    Failed,
    RolledBack,
    InDoubt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceApplyResult {
    pub device_id: DeviceId,
    pub changed: bool,
    pub status: ApplyStatus,
    pub tx_id: Option<String>,
    pub strategy: Option<TransactionStrategy>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_report: Option<DeviceVerifyReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyIntentResponse {
    pub request_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub reused: bool,
    pub tx_id: Option<String>,
    pub status: ApplyStatus,
    pub strategy: Option<TransactionStrategy>,
    pub device_results: Vec<DeviceApplyResult>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_report: Option<ApplyVerifyReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceVerifyStatus {
    Passed,
    Failed,
    Skipped,
    InDoubt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyVerifyStatus {
    Passed,
    Failed,
    Partial,
    Skipped,
    InDoubt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyScopeSummary {
    pub vlan_count: usize,
    pub interface_count: usize,
    pub acl_count: usize,
    pub acl_binding_count: usize,
    pub delete_vlan_count: usize,
    pub delete_interface_count: usize,
    pub delete_acl_count: usize,
    pub delete_acl_binding_count: usize,
}

impl VerifyScopeSummary {
    pub fn from_change_set(change_set: &ChangeSet) -> Self {
        let mut vlans = BTreeSet::new();
        let mut interfaces = BTreeSet::new();
        let mut acls = BTreeSet::new();
        let mut acl_bindings = BTreeSet::new();
        let mut delete_vlans = BTreeSet::new();
        let mut delete_interfaces = BTreeSet::new();
        let mut delete_acls = BTreeSet::new();
        let mut delete_acl_bindings = BTreeSet::new();

        for op in &change_set.ops {
            match op {
                ChangeOp::CreateVlan(vlan) => {
                    vlans.insert(vlan.vlan_id);
                }
                ChangeOp::UpdateVlan { after, .. } => {
                    vlans.insert(after.vlan_id);
                }
                ChangeOp::DeleteVlan { vlan_id } => {
                    delete_vlans.insert(*vlan_id);
                }
                ChangeOp::UpdateInterface { after, .. } => {
                    interfaces.insert(after.name.clone());
                }
                ChangeOp::DeleteInterfaceConfig { interface_name } => {
                    delete_interfaces.insert(interface_name.clone());
                }
                ChangeOp::CreateAcl(acl) => {
                    acls.insert(acl.acl_id);
                }
                ChangeOp::UpdateAcl { after, .. } => {
                    acls.insert(after.acl_id);
                }
                ChangeOp::DeleteAcl { acl_id } => {
                    delete_acls.insert(*acl_id);
                }
                ChangeOp::CreateAclBinding(binding) => {
                    acl_bindings.insert(binding.key());
                }
                ChangeOp::UpdateAclBinding { after, .. } => {
                    acl_bindings.insert(after.key());
                }
                ChangeOp::DeleteAclBinding {
                    interface_name,
                    direction,
                    acl_id,
                } => {
                    delete_acl_bindings.insert(format!(
                        "{}|{}",
                        acl_binding_key(interface_name, direction),
                        acl_id
                    ));
                }
            }
        }

        Self {
            vlan_count: vlans.len(),
            interface_count: interfaces.len(),
            acl_count: acls.len(),
            acl_binding_count: acl_bindings.len(),
            delete_vlan_count: delete_vlans.len(),
            delete_interface_count: delete_interfaces.len(),
            delete_acl_count: delete_acls.len(),
            delete_acl_binding_count: delete_acl_bindings.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceVerifyReport {
    pub device_id: DeviceId,
    pub status: DeviceVerifyStatus,
    pub source: String,
    pub scope: VerifyScopeSummary,
    pub warnings: Vec<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

impl DeviceVerifyReport {
    pub fn passed(
        device_id: DeviceId,
        scope: VerifyScopeSummary,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            device_id,
            status: DeviceVerifyStatus::Passed,
            source: "adapter_scoped_verify".into(),
            scope,
            warnings,
            error_code: None,
            error_message: None,
        }
    }

    pub fn failed(
        device_id: DeviceId,
        scope: VerifyScopeSummary,
        error_code: String,
        error_message: String,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            device_id,
            status: DeviceVerifyStatus::Failed,
            source: "adapter_scoped_verify".into(),
            scope,
            warnings,
            error_code: Some(error_code),
            error_message: Some(error_message),
        }
    }

    pub fn skipped(device_id: DeviceId) -> Self {
        Self {
            device_id,
            status: DeviceVerifyStatus::Skipped,
            source: "adapter_scoped_verify".into(),
            scope: VerifyScopeSummary::default(),
            warnings: Vec::new(),
            error_code: None,
            error_message: None,
        }
    }

    pub fn in_doubt(device_id: DeviceId, error_code: String, error_message: String) -> Self {
        Self {
            device_id,
            status: DeviceVerifyStatus::InDoubt,
            source: "adapter_scoped_verify".into(),
            scope: VerifyScopeSummary::default(),
            warnings: Vec::new(),
            error_code: Some(error_code),
            error_message: Some(error_message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyVerifyReport {
    pub status: ApplyVerifyStatus,
    pub passed: Vec<DeviceId>,
    pub failed: Vec<DeviceId>,
    pub skipped: Vec<DeviceId>,
    pub in_doubt: Vec<DeviceId>,
    pub attention_required: bool,
    pub warning_count: usize,
}

impl ApplyVerifyReport {
    pub fn from_device_results(device_results: &[DeviceApplyResult]) -> Self {
        let mut passed = Vec::new();
        let mut failed = Vec::new();
        let mut skipped = Vec::new();
        let mut in_doubt = Vec::new();
        let mut warning_count = 0;

        for report in device_results
            .iter()
            .filter_map(|result| result.verify_report.as_ref())
        {
            warning_count += report.warnings.len();
            match report.status {
                DeviceVerifyStatus::Passed => passed.push(report.device_id.clone()),
                DeviceVerifyStatus::Failed => failed.push(report.device_id.clone()),
                DeviceVerifyStatus::Skipped => skipped.push(report.device_id.clone()),
                DeviceVerifyStatus::InDoubt => in_doubt.push(report.device_id.clone()),
            }
        }

        let status = if !in_doubt.is_empty() {
            ApplyVerifyStatus::InDoubt
        } else if !failed.is_empty() && !passed.is_empty() {
            ApplyVerifyStatus::Partial
        } else if !failed.is_empty() {
            ApplyVerifyStatus::Failed
        } else if !passed.is_empty() {
            ApplyVerifyStatus::Passed
        } else {
            ApplyVerifyStatus::Skipped
        };

        Self {
            status,
            passed,
            failed,
            skipped,
            in_doubt,
            attention_required: !failed.is_empty() || !in_doubt.is_empty(),
            warning_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResponse {
    pub device_results: Vec<DeviceApplyResult>,
    pub change_sets: Vec<ChangeSet>,
    #[serde(default)]
    pub change_plans: Vec<ChangePlan>,
    pub noop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshStateResponse {
    pub refreshed_devices: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceOnboardingResponse {
    pub device_id: DeviceId,
    pub lifecycle_state: DeviceLifecycleState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAuditResponse {
    pub drifted_devices: Vec<DeviceId>,
}
