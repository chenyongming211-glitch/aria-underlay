use crate::device::capability::BackendKind;
use crate::device::{DeviceCapabilityProfile, DeviceInfo};
use crate::model::{AdminState, DeviceId, InterfaceConfig, PortMode, Vendor, VlanConfig};
use crate::planner::device_plan::DeviceDesiredState;
use crate::proto::adapter;
use crate::state::DeviceShadowState;
use crate::tx::{choose_strategy, CapabilityFlags, TransactionMode};
use crate::{AdapterErrorDetail, UnderlayError, UnderlayResult};

pub fn extract_adapter_errors(errors: Vec<adapter::AdapterError>) -> Option<UnderlayError> {
    let mut errors = errors;
    if errors.is_empty() {
        return None;
    }
    let first = errors.remove(0);
    let additional = errors
        .into_iter()
        .map(|e| AdapterErrorDetail {
            code: e.code,
            message: e.message,
        })
        .collect();
    Some(UnderlayError::AdapterOperation {
        code: first.code,
        message: first.message,
        retryable: first.retryable,
        errors: additional,
    })
}

pub fn device_ref_from_info(info: &DeviceInfo) -> adapter::DeviceRef {
    adapter::DeviceRef {
        device_id: info.id.0.clone(),
        management_ip: info.management_ip.clone(),
        management_port: u32::from(info.management_port),
        vendor_hint: vendor_to_proto(info.vendor_hint.unwrap_or(Vendor::Unknown)) as i32,
        model_hint: info.model_hint.clone().unwrap_or_default(),
        secret_ref: info.secret_ref.clone(),
    }
}

pub fn capability_from_proto(proto: adapter::DeviceCapability, warnings: Vec<String>) -> DeviceCapabilityProfile {
    let supported_backends = proto
        .supported_backends
        .into_iter()
        .filter_map(backend_from_i32)
        .collect::<Vec<_>>();

    let flags = CapabilityFlags {
        supports_candidate: proto.supports_candidate,
        supports_validate: proto.supports_validate,
        supports_confirmed_commit: proto.supports_confirmed_commit,
        supports_rollback_on_error: proto.supports_rollback_on_error,
        supports_writable_running: proto.supports_writable_running,
        supports_cli_fallback: supported_backends
            .iter()
            .any(|backend| matches!(backend, BackendKind::Cli | BackendKind::Netmiko)),
    };

    let recommended_strategy = choose_strategy(flags, TransactionMode::AllowBestEffortCli);

    DeviceCapabilityProfile {
        vendor: vendor_from_i32(proto.vendor),
        model: empty_to_none(proto.model),
        os_version: empty_to_none(proto.os_version),
        raw_capabilities: proto.raw_capabilities,
        supports_netconf: proto.supports_netconf,
        supports_candidate: proto.supports_candidate,
        supports_validate: proto.supports_validate,
        supports_confirmed_commit: proto.supports_confirmed_commit,
        supports_persist_id: proto.supports_persist_id,
        supports_rollback_on_error: proto.supports_rollback_on_error,
        supports_writable_running: proto.supports_writable_running,
        supported_backends,
        recommended_strategy,
        warnings,
    }
}

pub fn desired_state_to_proto(desired: &DeviceDesiredState) -> adapter::DesiredDeviceState {
    adapter::DesiredDeviceState {
        device_id: desired.device_id.0.clone(),
        vlans: desired
            .vlans
            .values()
            .map(|vlan| adapter::VlanConfig {
                vlan_id: u32::from(vlan.vlan_id),
                name: vlan.name.clone(),
                description: vlan.description.clone(),
            })
            .collect(),
        interfaces: desired
            .interfaces
            .values()
            .map(interface_to_proto)
            .collect(),
    }
}

pub fn shadow_state_from_proto(proto: adapter::ObservedDeviceState, warnings: Vec<String>) -> UnderlayResult<DeviceShadowState> {
    let mut vlans = std::collections::BTreeMap::new();
    for vlan in proto.vlans {
        let vlan_id = u16::try_from(vlan.vlan_id).map_err(|_| {
            UnderlayError::AdapterOperation {
                code: "INVALID_VLAN_ID".into(),
                message: format!("adapter returned invalid VLAN id {}", vlan.vlan_id),
                retryable: false,
                errors: Vec::new(),
            }
        })?;
        vlans.insert(
            vlan_id,
            VlanConfig {
                vlan_id,
                name: vlan.name,
                description: vlan.description,
            },
        );
    }

    let mut interfaces = std::collections::BTreeMap::new();
    for iface in proto.interfaces {
        let interface = interface_from_proto(iface)?;
        interfaces.insert(interface.name.clone(), interface);
    }

    Ok(DeviceShadowState {
        device_id: DeviceId(proto.device_id),
        revision: 0,
        vlans,
        interfaces,
        warnings,
    })
}

pub fn adapter_result_to_outcome(proto: adapter::AdapterResult) -> UnderlayResult<AdapterOutcome> {
    if let Some(error) = extract_adapter_errors(proto.errors) {
        return Err(error);
    }

    Ok(AdapterOutcome {
        status: adapter_status_from_i32(proto.status),
        changed: proto.changed,
        warnings: proto.warnings,
        normalized_state: proto
            .normalized_state
            .map(|state| shadow_state_from_proto(state, Vec::new()))
            .transpose()?,
    })
}

#[derive(Debug, Clone)]
pub struct AdapterOutcome {
    pub status: AdapterOperationStatus,
    pub changed: bool,
    pub warnings: Vec<String>,
    pub normalized_state: Option<DeviceShadowState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterOperationStatus {
    Unspecified,
    NoChange,
    Prepared,
    Committed,
    RolledBack,
    Failed,
    InDoubt,
}

fn vendor_to_proto(vendor: Vendor) -> adapter::Vendor {
    match vendor {
        Vendor::Huawei => adapter::Vendor::Huawei,
        Vendor::H3c => adapter::Vendor::H3c,
        Vendor::Cisco => adapter::Vendor::Cisco,
        Vendor::Ruijie => adapter::Vendor::Ruijie,
        Vendor::Unknown => adapter::Vendor::Unknown,
    }
}

fn vendor_from_i32(value: i32) -> Vendor {
    match adapter::Vendor::try_from(value).unwrap_or(adapter::Vendor::Unknown) {
        adapter::Vendor::Huawei => Vendor::Huawei,
        adapter::Vendor::H3c => Vendor::H3c,
        adapter::Vendor::Cisco => Vendor::Cisco,
        adapter::Vendor::Ruijie => Vendor::Ruijie,
        _ => Vendor::Unknown,
    }
}

fn backend_from_i32(value: i32) -> Option<BackendKind> {
    match adapter::BackendKind::try_from(value).ok()? {
        adapter::BackendKind::Netconf => Some(BackendKind::Netconf),
        adapter::BackendKind::Napalm => Some(BackendKind::Napalm),
        adapter::BackendKind::Netmiko => Some(BackendKind::Netmiko),
        adapter::BackendKind::Cli => Some(BackendKind::Cli),
        _ => None,
    }
}

fn interface_to_proto(interface: &InterfaceConfig) -> adapter::InterfaceConfig {
    adapter::InterfaceConfig {
        name: interface.name.clone(),
        admin_state: match interface.admin_state {
            AdminState::Up => adapter::AdminState::Up as i32,
            AdminState::Down => adapter::AdminState::Down as i32,
        },
        description: interface.description.clone(),
        mode: Some(port_mode_to_proto(&interface.mode)),
    }
}

fn port_mode_to_proto(mode: &PortMode) -> adapter::PortMode {
    match mode {
        PortMode::Access { vlan_id } => adapter::PortMode {
            kind: adapter::PortModeKind::Access as i32,
            access_vlan: Some(u32::from(*vlan_id)),
            native_vlan: None,
            allowed_vlans: Vec::new(),
        },
        PortMode::Trunk {
            native_vlan,
            allowed_vlans,
        } => adapter::PortMode {
            kind: adapter::PortModeKind::Trunk as i32,
            access_vlan: None,
            native_vlan: native_vlan.map(u32::from),
            allowed_vlans: allowed_vlans.iter().map(|vlan| u32::from(*vlan)).collect(),
        },
    }
}

fn interface_from_proto(proto: adapter::InterfaceConfig) -> UnderlayResult<InterfaceConfig> {
    let mode = proto.mode.ok_or_else(|| UnderlayError::AdapterOperation {
        code: "MISSING_PORT_MODE".into(),
        message: format!("adapter returned interface {} without port mode", proto.name),
        retryable: false,
        errors: Vec::new(),
    })?;

    Ok(InterfaceConfig {
        name: proto.name,
        admin_state: match adapter::AdminState::try_from(proto.admin_state)
            .unwrap_or(adapter::AdminState::Unspecified)
        {
            adapter::AdminState::Up => AdminState::Up,
            adapter::AdminState::Down => AdminState::Down,
            _ => {
                return Err(UnderlayError::AdapterOperation {
                    code: "INVALID_ADMIN_STATE".into(),
                    message: "adapter returned invalid admin state".into(),
                    retryable: false,
                    errors: Vec::new(),
                })
            }
        },
        description: proto.description,
        mode: port_mode_from_proto(mode)?,
    })
}

fn port_mode_from_proto(proto: adapter::PortMode) -> UnderlayResult<PortMode> {
    match adapter::PortModeKind::try_from(proto.kind).unwrap_or(adapter::PortModeKind::Unspecified)
    {
        adapter::PortModeKind::Access => {
            let vlan_id = proto.access_vlan.ok_or_else(|| UnderlayError::AdapterOperation {
                code: "MISSING_ACCESS_VLAN".into(),
                message: "adapter returned access port without access vlan".into(),
                retryable: false,
                errors: Vec::new(),
            })?;
            Ok(PortMode::Access {
                vlan_id: u16::try_from(vlan_id).map_err(|_| UnderlayError::AdapterOperation {
                    code: "INVALID_VLAN_ID".into(),
                    message: format!("adapter returned invalid access VLAN id {vlan_id}"),
                    retryable: false,
                    errors: Vec::new(),
                })?,
            })
        }
        adapter::PortModeKind::Trunk => Ok(PortMode::Trunk {
            native_vlan: proto
                .native_vlan
                .map(|vlan| {
                    u16::try_from(vlan).map_err(|_| UnderlayError::AdapterOperation {
                        code: "INVALID_VLAN_ID".into(),
                        message: format!("adapter returned invalid native VLAN id {vlan}"),
                        retryable: false,
                        errors: Vec::new(),
                    })
                })
                .transpose()?,
            allowed_vlans: proto
                .allowed_vlans
                .into_iter()
                .map(|vlan| {
                    u16::try_from(vlan).map_err(|_| UnderlayError::AdapterOperation {
                        code: "INVALID_VLAN_ID".into(),
                        message: format!("adapter returned invalid allowed VLAN id {vlan}"),
                        retryable: false,
                        errors: Vec::new(),
                    })
                })
                .collect::<UnderlayResult<Vec<_>>>()?,
        }),
        _ => Err(UnderlayError::AdapterOperation {
            code: "INVALID_PORT_MODE".into(),
            message: "adapter returned invalid port mode".into(),
            retryable: false,
            errors: Vec::new(),
        }),
    }
}

fn adapter_status_from_i32(value: i32) -> AdapterOperationStatus {
    match adapter::AdapterOperationStatus::try_from(value)
        .unwrap_or(adapter::AdapterOperationStatus::Unspecified)
    {
        adapter::AdapterOperationStatus::NoChange => AdapterOperationStatus::NoChange,
        adapter::AdapterOperationStatus::Prepared => AdapterOperationStatus::Prepared,
        adapter::AdapterOperationStatus::Committed => AdapterOperationStatus::Committed,
        adapter::AdapterOperationStatus::RolledBack => AdapterOperationStatus::RolledBack,
        adapter::AdapterOperationStatus::Failed => AdapterOperationStatus::Failed,
        adapter::AdapterOperationStatus::InDoubt => AdapterOperationStatus::InDoubt,
        _ => AdapterOperationStatus::Unspecified,
    }
}

fn empty_to_none(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
