use crate::device::capability::BackendKind;
use crate::device::{DeviceCapabilityProfile, DeviceInfo};
use crate::model::Vendor;
use crate::proto::adapter;
use crate::tx::{choose_strategy, CapabilityFlags, TransactionMode};

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

pub fn capability_from_proto(proto: adapter::DeviceCapability) -> DeviceCapabilityProfile {
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
    }
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

fn empty_to_none(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
