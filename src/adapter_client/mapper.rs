use crate::device::{
    capability::BackendKind, DeviceCapabilityProfile, DeviceInfo, DeviceModelProfile,
    HostKeyPolicy,
};
use crate::device::model_profile::YangModuleSummary;
use crate::engine::diff::{ChangeOp, ChangeSet};
use crate::model::{
    AclAction, AclBinding, AclConfig, AclDirection, AclEndpoint, AclKind, AclProtocol, AclRule,
    AdminState, DeviceId, InterfaceConfig, PortMode, Vendor, VlanConfig,
};
use crate::planner::device_plan::DeviceDesiredState;
use crate::proto::adapter;
use crate::state::DeviceShadowState;
use crate::tx::{
    choose_strategy, CapabilityFlags, RecoveryAction, TransactionMode, TransactionStrategy,
};
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
    let (host_key_policy, known_hosts_path, pinned_host_key_fingerprint) =
        host_key_policy_to_proto(&info.host_key_policy);

    adapter::DeviceRef {
        device_id: info.id.0.clone(),
        management_ip: info.management_ip.clone(),
        management_port: u32::from(info.management_port),
        vendor_hint: vendor_to_proto(info.vendor_hint.unwrap_or(Vendor::Unknown)) as i32,
        model_hint: info.model_hint.clone().unwrap_or_default(),
        secret_ref: info.secret_ref.clone(),
        host_key_policy: host_key_policy as i32,
        known_hosts_path,
        pinned_host_key_fingerprint,
    }
}

fn host_key_policy_to_proto(
    policy: &HostKeyPolicy,
) -> (adapter::HostKeyPolicy, String, String) {
    match policy {
        HostKeyPolicy::TrustOnFirstUse => (
            adapter::HostKeyPolicy::TrustOnFirstUse,
            String::new(),
            String::new(),
        ),
        HostKeyPolicy::KnownHostsFile { path } => (
            adapter::HostKeyPolicy::KnownHostsFile,
            path.clone(),
            String::new(),
        ),
        HostKeyPolicy::PinnedKey { fingerprint } => (
            adapter::HostKeyPolicy::PinnedKey,
            String::new(),
            fingerprint.clone(),
        ),
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
        supports_persist_id: proto.supports_persist_id,
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
        model_profile: proto.model_profile.map(DeviceModelProfile::from_proto),
        recommended_strategy,
        warnings,
        yang_modules: proto
            .yang_modules
            .into_iter()
            .map(YangModuleSummary::from_proto)
            .collect(),
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
        acls: desired.acls.values().map(acl_to_proto).collect(),
        acl_bindings: desired
            .acl_bindings
            .values()
            .map(acl_binding_to_proto)
            .collect(),
        delete_vlan_ids: desired
            .delete_vlan_ids
            .iter()
            .map(|vlan_id| u32::from(*vlan_id))
            .collect(),
        delete_acl_ids: desired
            .delete_acl_ids
            .iter()
            .map(|acl_id| u32::from(*acl_id))
            .collect(),
        delete_acl_bindings: desired
            .delete_acl_bindings
            .values()
            .map(acl_binding_to_proto)
            .collect(),
    }
}

pub fn state_scope_from_desired(desired: &DeviceDesiredState) -> adapter::StateScope {
    let interface_names = desired
        .interfaces
        .keys()
        .cloned()
        .chain(
            desired
                .acl_bindings
                .values()
                .map(|binding| binding.interface_name.clone()),
        )
        .chain(
            desired
                .delete_acl_bindings
                .values()
                .map(|binding| binding.interface_name.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>();

    adapter::StateScope {
        full: false,
        vlan_ids: desired
            .vlans
            .keys()
            .map(|vlan_id| u32::from(*vlan_id))
            .chain(desired.delete_vlan_ids.iter().map(|vlan_id| u32::from(*vlan_id)))
            .collect(),
        interface_names: interface_names.into_iter().collect(),
        acl_ids: desired
            .acls
            .keys()
            .copied()
            .chain(desired.acl_bindings.values().map(|binding| binding.acl_id))
            .chain(desired.delete_acl_ids.iter().copied())
            .chain(
                desired
                    .delete_acl_bindings
                    .values()
                    .map(|binding| binding.acl_id),
            )
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(u32::from)
            .collect(),
    }
}

pub fn state_scope_from_change_set(change_set: &ChangeSet) -> adapter::StateScope {
    let mut vlan_ids = std::collections::BTreeSet::new();
    let mut interface_names = std::collections::BTreeSet::new();
    let mut acl_ids = std::collections::BTreeSet::new();

    for op in &change_set.ops {
        match op {
            ChangeOp::CreateVlan(vlan) => {
                vlan_ids.insert(u32::from(vlan.vlan_id));
            }
            ChangeOp::UpdateVlan { before, after } => {
                vlan_ids.insert(u32::from(before.vlan_id));
                vlan_ids.insert(u32::from(after.vlan_id));
            }
            ChangeOp::DeleteVlan { vlan_id } => {
                vlan_ids.insert(u32::from(*vlan_id));
            }
            ChangeOp::UpdateInterface { before, after } => {
                if let Some(before) = before {
                    interface_names.insert(before.name.clone());
                }
                interface_names.insert(after.name.clone());
            }
            ChangeOp::CreateAcl(acl) => {
                acl_ids.insert(u32::from(acl.acl_id));
            }
            ChangeOp::UpdateAcl { before, after } => {
                acl_ids.insert(u32::from(before.acl_id));
                acl_ids.insert(u32::from(after.acl_id));
            }
            ChangeOp::DeleteAcl { acl_id } => {
                acl_ids.insert(u32::from(*acl_id));
            }
            ChangeOp::CreateAclBinding(binding) => {
                interface_names.insert(binding.interface_name.clone());
                acl_ids.insert(u32::from(binding.acl_id));
            }
            ChangeOp::UpdateAclBinding { before, after } => {
                interface_names.insert(before.interface_name.clone());
                interface_names.insert(after.interface_name.clone());
                acl_ids.insert(u32::from(before.acl_id));
                acl_ids.insert(u32::from(after.acl_id));
            }
            ChangeOp::DeleteAclBinding {
                interface_name,
                direction: _,
                acl_id,
            } => {
                interface_names.insert(interface_name.clone());
                acl_ids.insert(u32::from(*acl_id));
            }
        }
    }

    adapter::StateScope {
        full: false,
        vlan_ids: vlan_ids.into_iter().collect(),
        interface_names: interface_names.into_iter().collect(),
        acl_ids: acl_ids.into_iter().collect(),
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
        if vlan_id < 1 || vlan_id > 4094 {
            return Err(UnderlayError::AdapterOperation {
                code: "INVALID_VLAN_ID".into(),
                message: format!(
                    "adapter returned out-of-range VLAN id {} (valid: 1–4094)",
                    vlan_id
                ),
                retryable: false,
                errors: Vec::new(),
            });
        }
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

    let mut acls = std::collections::BTreeMap::new();
    for acl in proto.acls {
        let acl = acl_from_proto(acl)?;
        acls.insert(acl.acl_id, acl);
    }

    let mut acl_bindings = std::collections::BTreeMap::new();
    for binding in proto.acl_bindings {
        let binding = acl_binding_from_proto(binding)?;
        acl_bindings.insert(binding.key(), binding);
    }

    Ok(DeviceShadowState {
        device_id: DeviceId(proto.device_id),
        revision: 0,
        vlans,
        interfaces,
        acls,
        acl_bindings,
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
        prepared_candidate_checksum: empty_to_none(proto.prepared_candidate_checksum),
        normalized_state: proto
            .normalized_state
            .map(|state| shadow_state_from_proto(state, Vec::new()))
            .transpose()?,
    })
}

pub fn strategy_to_proto(strategy: TransactionStrategy) -> adapter::TransactionStrategy {
    match strategy {
        TransactionStrategy::ConfirmedCommit => adapter::TransactionStrategy::ConfirmedCommit,
        TransactionStrategy::CandidateCommit => adapter::TransactionStrategy::CandidateCommit,
        TransactionStrategy::RunningRollbackOnError => {
            adapter::TransactionStrategy::RunningRollbackOnError
        }
        TransactionStrategy::BestEffortCli => adapter::TransactionStrategy::BestEffortCli,
        TransactionStrategy::Unsupported => adapter::TransactionStrategy::Unsupported,
    }
}

pub fn recovery_action_to_proto(action: RecoveryAction) -> adapter::RecoveryAction {
    match action {
        RecoveryAction::DiscardPreparedChanges => {
            adapter::RecoveryAction::DiscardPreparedChanges
        }
        RecoveryAction::AdapterRecover => adapter::RecoveryAction::AdapterRecover,
        RecoveryAction::Noop | RecoveryAction::ManualIntervention => {
            adapter::RecoveryAction::Unspecified
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdapterOutcome {
    pub status: AdapterOperationStatus,
    pub changed: bool,
    pub warnings: Vec<String>,
    pub prepared_candidate_checksum: Option<String>,
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
    ConfirmedCommitPending,
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

fn acl_to_proto(acl: &AclConfig) -> adapter::AclConfig {
    adapter::AclConfig {
        acl_id: u32::from(acl.acl_id),
        name: acl.name.clone(),
        description: acl.description.clone(),
        rules: acl.rules.iter().map(acl_rule_to_proto).collect(),
        kind: acl_kind_to_proto(&acl.kind) as i32,
    }
}

fn acl_binding_to_proto(binding: &AclBinding) -> adapter::AclBinding {
    adapter::AclBinding {
        interface_name: binding.interface_name.clone(),
        direction: acl_direction_to_proto(&binding.direction) as i32,
        acl_id: u32::from(binding.acl_id),
    }
}

fn acl_rule_to_proto(rule: &AclRule) -> adapter::AclRule {
    adapter::AclRule {
        sequence: u32::from(rule.sequence),
        action: acl_action_to_proto(&rule.action) as i32,
        protocol: acl_protocol_to_proto(&rule.protocol) as i32,
        source: rule.source.as_ref().map(acl_endpoint_to_proto),
        destination: rule.destination.as_ref().map(acl_endpoint_to_proto),
        source_port_eq: rule.source_port_eq.map(u32::from),
        destination_port_eq: rule.destination_port_eq.map(u32::from),
        description: rule.description.clone(),
    }
}

fn acl_endpoint_to_proto(endpoint: &AclEndpoint) -> adapter::AclEndpoint {
    adapter::AclEndpoint {
        address: endpoint.address.clone(),
        wildcard: endpoint.wildcard.clone(),
    }
}

fn acl_from_proto(proto: adapter::AclConfig) -> UnderlayResult<AclConfig> {
    let acl_id = acl_id_from_u32(proto.acl_id, "adapter returned invalid ACL id")?;
    Ok(AclConfig {
        acl_id,
        kind: acl_kind_from_i32(proto.kind, acl_id)?,
        name: proto.name,
        description: proto.description,
        rules: proto
            .rules
            .into_iter()
            .map(acl_rule_from_proto)
            .collect::<UnderlayResult<Vec<_>>>()?,
    })
}

fn acl_kind_to_proto(kind: &AclKind) -> adapter::AclKind {
    match kind {
        AclKind::AdvancedIpv4 => adapter::AclKind::AdvancedIpv4,
        AclKind::BasicIpv4 => adapter::AclKind::BasicIpv4,
    }
}

fn acl_kind_from_i32(value: i32, acl_id: u16) -> UnderlayResult<AclKind> {
    match adapter::AclKind::try_from(value).unwrap_or(adapter::AclKind::Unspecified) {
        adapter::AclKind::AdvancedIpv4 if (3000..=3999).contains(&acl_id) => {
            Ok(AclKind::AdvancedIpv4)
        }
        adapter::AclKind::BasicIpv4 if (2000..=2999).contains(&acl_id) => {
            Ok(AclKind::BasicIpv4)
        }
        adapter::AclKind::Unspecified if (2000..=2999).contains(&acl_id) => {
            Ok(AclKind::BasicIpv4)
        }
        adapter::AclKind::Unspecified if (3000..=3999).contains(&acl_id) => {
            Ok(AclKind::AdvancedIpv4)
        }
        _ => Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_KIND".into(),
            message: "adapter returned invalid ACL kind".into(),
            retryable: false,
            errors: Vec::new(),
        }),
    }
}

fn acl_binding_from_proto(proto: adapter::AclBinding) -> UnderlayResult<AclBinding> {
    if proto.interface_name.trim().is_empty() {
        return Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_BINDING".into(),
            message: "adapter returned ACL binding without interface_name".into(),
            retryable: false,
            errors: Vec::new(),
        });
    }
    Ok(AclBinding {
        interface_name: proto.interface_name,
        direction: acl_direction_from_i32(proto.direction)?,
        acl_id: acl_id_from_u32(proto.acl_id, "adapter returned invalid ACL binding ACL id")?,
    })
}

fn acl_rule_from_proto(proto: adapter::AclRule) -> UnderlayResult<AclRule> {
    Ok(AclRule {
        sequence: u16::try_from(proto.sequence).map_err(|_| UnderlayError::AdapterOperation {
            code: "INVALID_ACL_RULE_SEQUENCE".into(),
            message: format!("adapter returned invalid ACL rule sequence {}", proto.sequence),
            retryable: false,
            errors: Vec::new(),
        })?,
        action: acl_action_from_i32(proto.action)?,
        protocol: acl_protocol_from_i32(proto.protocol)?,
        source: proto.source.map(acl_endpoint_from_proto).transpose()?,
        destination: proto.destination.map(acl_endpoint_from_proto).transpose()?,
        source_port_eq: proto
            .source_port_eq
            .map(|port| acl_port_from_u32(port, "source_port_eq"))
            .transpose()?,
        destination_port_eq: proto
            .destination_port_eq
            .map(|port| acl_port_from_u32(port, "destination_port_eq"))
            .transpose()?,
        description: proto.description,
    })
}

fn acl_endpoint_from_proto(proto: adapter::AclEndpoint) -> UnderlayResult<AclEndpoint> {
    if proto.address.trim().is_empty() || proto.wildcard.trim().is_empty() {
        return Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_ENDPOINT".into(),
            message: "adapter returned ACL endpoint without address or wildcard".into(),
            retryable: false,
            errors: Vec::new(),
        });
    }
    Ok(AclEndpoint {
        address: proto.address,
        wildcard: proto.wildcard,
    })
}

fn acl_action_to_proto(action: &AclAction) -> adapter::AclAction {
    match action {
        AclAction::Permit => adapter::AclAction::Permit,
        AclAction::Deny => adapter::AclAction::Deny,
    }
}

fn acl_action_from_i32(value: i32) -> UnderlayResult<AclAction> {
    match adapter::AclAction::try_from(value).unwrap_or(adapter::AclAction::Unspecified) {
        adapter::AclAction::Permit => Ok(AclAction::Permit),
        adapter::AclAction::Deny => Ok(AclAction::Deny),
        _ => Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_ACTION".into(),
            message: "adapter returned invalid ACL action".into(),
            retryable: false,
            errors: Vec::new(),
        }),
    }
}

fn acl_protocol_to_proto(protocol: &AclProtocol) -> adapter::AclProtocol {
    match protocol {
        AclProtocol::Ip => adapter::AclProtocol::Ip,
        AclProtocol::Tcp => adapter::AclProtocol::Tcp,
        AclProtocol::Udp => adapter::AclProtocol::Udp,
        AclProtocol::Icmp => adapter::AclProtocol::Icmp,
    }
}

fn acl_protocol_from_i32(value: i32) -> UnderlayResult<AclProtocol> {
    match adapter::AclProtocol::try_from(value).unwrap_or(adapter::AclProtocol::Unspecified) {
        adapter::AclProtocol::Ip => Ok(AclProtocol::Ip),
        adapter::AclProtocol::Tcp => Ok(AclProtocol::Tcp),
        adapter::AclProtocol::Udp => Ok(AclProtocol::Udp),
        adapter::AclProtocol::Icmp => Ok(AclProtocol::Icmp),
        _ => Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_PROTOCOL".into(),
            message: "adapter returned invalid ACL protocol".into(),
            retryable: false,
            errors: Vec::new(),
        }),
    }
}

fn acl_direction_to_proto(direction: &AclDirection) -> adapter::AclDirection {
    match direction {
        AclDirection::Inbound => adapter::AclDirection::Inbound,
        AclDirection::Outbound => adapter::AclDirection::Outbound,
    }
}

fn acl_direction_from_i32(value: i32) -> UnderlayResult<AclDirection> {
    match adapter::AclDirection::try_from(value).unwrap_or(adapter::AclDirection::Unspecified) {
        adapter::AclDirection::Inbound => Ok(AclDirection::Inbound),
        adapter::AclDirection::Outbound => Ok(AclDirection::Outbound),
        _ => Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_DIRECTION".into(),
            message: "adapter returned invalid ACL direction".into(),
            retryable: false,
            errors: Vec::new(),
        }),
    }
}

fn acl_id_from_u32(value: u32, message: &str) -> UnderlayResult<u16> {
    let acl_id = u16::try_from(value).map_err(|_| UnderlayError::AdapterOperation {
        code: "INVALID_ACL_ID".into(),
        message: format!("{message} {value}"),
        retryable: false,
        errors: Vec::new(),
    })?;
    if !(2000..=3999).contains(&acl_id) {
        return Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_ID".into(),
            message: format!("adapter returned out-of-range numeric IPv4 ACL id {acl_id}"),
            retryable: false,
            errors: Vec::new(),
        });
    }
    Ok(acl_id)
}

fn acl_port_from_u32(value: u32, field: &str) -> UnderlayResult<u16> {
    let port = u16::try_from(value).map_err(|_| UnderlayError::AdapterOperation {
        code: "INVALID_ACL_PORT".into(),
        message: format!("adapter returned invalid ACL {field} {value}"),
        retryable: false,
        errors: Vec::new(),
    })?;
    if port == 0 {
        return Err(UnderlayError::AdapterOperation {
            code: "INVALID_ACL_PORT".into(),
            message: format!("adapter returned invalid ACL {field} 0"),
            retryable: false,
            errors: Vec::new(),
        });
    }
    Ok(port)
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
        adapter::AdapterOperationStatus::ConfirmedCommitPending => {
            AdapterOperationStatus::ConfirmedCommitPending
        }
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
