use std::collections::BTreeSet;
use std::io;

use aria_underlay::api::request::{
    ApplyDomainIntentRequest, ApplyOptions, ApplyReconcileMode,
};
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInventory, HostKeyPolicy, RegisterDeviceRequest};
use aria_underlay::engine::diff::ChangeOp;
use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    AclBindingIntent, AclIntent, ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent,
    UnderlayTopology,
};
use aria_underlay::model::{
    AclAction, AclDirection, AclEndpoint, AclKind, AclProtocol, AclRule, AdminState, DeviceId,
    DeviceRole, PortMode, Vendor,
};
use aria_underlay::state::drift::DriftPolicy;

const WRITE_ACK: &str = "I_UNDERSTAND_THIS_WRITES_DEVICE";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("ARIA_UNDERLAY_REAL_APPLY_ACK").as_deref() != Ok(WRITE_ACK) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("set ARIA_UNDERLAY_REAL_APPLY_ACK={WRITE_ACK} to run a real device write"),
        )
        .into());
    }

    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let endpoint_id =
        std::env::var("ARIA_UNDERLAY_ENDPOINT_ID").unwrap_or_else(|_| "stack-mgmt".into());
    let member_id =
        std::env::var("ARIA_UNDERLAY_MEMBER_ID").unwrap_or_else(|_| "member-a".into());
    let management_ip = required_env("ARIA_UNDERLAY_MGMT_IP")?;
    let management_port = env_u16("ARIA_UNDERLAY_MGMT_PORT", 830)?;
    let secret_ref = std::env::var("ARIA_UNDERLAY_SECRET_REF")
        .unwrap_or_else(|_| "local/real-device".into());
    let test_vlan_was_set = optional_env("ARIA_UNDERLAY_TEST_VLAN").is_some();
    let test_vlan = env_u16("ARIA_UNDERLAY_TEST_VLAN", 4093)?;
    let vlan_name =
        std::env::var("ARIA_UNDERLAY_TEST_VLAN_NAME").unwrap_or_else(|_| "aria-test".into());
    let vlan_description = optional_env("ARIA_UNDERLAY_TEST_VLAN_DESCRIPTION");
    let allow_degraded = env_bool("ARIA_UNDERLAY_ALLOW_DEGRADED", true)?;

    let interfaces = desired_interfaces(&member_id, test_vlan)?;
    let acls = desired_acls()?;
    let acl_bindings = desired_acl_bindings(&member_id, &acls)?;
    let vlans = if interfaces.is_empty() && !test_vlan_was_set {
        Vec::new()
    } else {
        desired_vlans(test_vlan, vlan_name, vlan_description, &interfaces)
    };

    let inventory = DeviceInventory::default();
    let service = AriaUnderlayService::new(inventory.clone());
    let device_id = DeviceId(endpoint_id.clone());

    service
        .register_device(RegisterDeviceRequest {
            tenant_id: std::env::var("ARIA_UNDERLAY_TENANT_ID")
                .unwrap_or_else(|_| "tenant-a".into()),
            site_id: std::env::var("ARIA_UNDERLAY_SITE_ID")
                .unwrap_or_else(|_| "site-a".into()),
            device_id: device_id.clone(),
            management_ip: management_ip.clone(),
            management_port,
            vendor_hint: Some(Vendor::H3c),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref: secret_ref.clone(),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint: adapter_endpoint.clone(),
        })
        .await?;

    let request = ApplyDomainIntentRequest {
        request_id: std::env::var("ARIA_UNDERLAY_REQUEST_ID")
            .unwrap_or_else(|_| "real-domain-apply-probe".into()),
        trace_id: Some(
            std::env::var("ARIA_UNDERLAY_TRACE_ID")
                .unwrap_or_else(|_| "real-domain-apply-probe".into()),
        ),
        intent: UnderlayDomainIntent {
            domain_id: std::env::var("ARIA_UNDERLAY_DOMAIN_ID")
                .unwrap_or_else(|_| "real-domain".into()),
            topology: UnderlayTopology::StackSingleManagementIp,
            endpoints: vec![ManagementEndpointIntent {
                endpoint_id: endpoint_id.clone(),
                host: management_ip,
                port: management_port,
                secret_ref,
                vendor_hint: Some(Vendor::H3c),
                model_hint: None,
            }],
            members: vec![SwitchMemberIntent {
                member_id,
                role: Some(DeviceRole::LeafA),
                management_endpoint_id: endpoint_id,
            }],
            vlans,
            interfaces,
            acls,
            acl_bindings,
            delete_vlan_ids: vec![],
            delete_acl_ids: vec![],
            delete_acl_bindings: vec![],
        },
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: allow_degraded,
            reconcile_mode: ApplyReconcileMode::MergeUpsert,
            drift_policy: DriftPolicy::ReportOnly,
        },
    };

    let dry_run = service.dry_run_domain(request.clone()).await?;
    println!("real_apply_dry_run_noop={}", dry_run.noop);
    println!("real_apply_change_sets={:#?}", dry_run.change_sets);
    if dry_run
        .change_sets
        .iter()
        .flat_map(|change_set| &change_set.ops)
        .any(|op| {
            matches!(
                op,
                ChangeOp::DeleteVlan { .. }
                    | ChangeOp::DeleteInterfaceConfig { .. }
                    | ChangeOp::DeleteAcl { .. }
                    | ChangeOp::UpdateAcl { .. }
                    | ChangeOp::UpdateAclBinding { .. }
                    | ChangeOp::DeleteAclBinding { .. }
            )
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "dry-run planned a delete or existing ACL/binding update; refusing real device write",
        )
        .into());
    }
    ensure_requested_acls_are_creates(&dry_run.change_sets, &request.intent.acls)?;
    ensure_requested_acl_bindings_are_creates(
        &dry_run.change_sets,
        &request.intent.acl_bindings,
    )?;

    let response = service.apply_domain_intent(request).await?;
    println!("real_apply_status={:?}", response.status);
    println!("real_apply_strategy={:?}", response.strategy);
    println!("real_apply_device_results={:#?}", response.device_results);
    println!("real_apply_warnings={:?}", response.warnings);

    Ok(())
}

fn desired_acls() -> Result<Vec<AclIntent>, Box<dyn std::error::Error>> {
    let Some(acl_id) = optional_env("ARIA_UNDERLAY_TEST_ACL_ID") else {
        return Ok(Vec::new());
    };
    let acl_id = acl_id.parse::<u16>()?;
    let rule = AclRule {
        sequence: env_u16("ARIA_UNDERLAY_ACL_RULE_SEQUENCE", 10)?,
        action: acl_action(
            optional_env("ARIA_UNDERLAY_ACL_RULE_ACTION")
                .unwrap_or_else(|| "permit".into())
                .as_str(),
        )?,
        protocol: acl_protocol(
            optional_env("ARIA_UNDERLAY_ACL_RULE_PROTOCOL")
                .unwrap_or_else(|| "ip".into())
                .as_str(),
        )?,
        source: acl_endpoint(
            "ARIA_UNDERLAY_ACL_RULE_SOURCE",
            "ARIA_UNDERLAY_ACL_RULE_SOURCE_WILDCARD",
        )?,
        destination: acl_endpoint(
            "ARIA_UNDERLAY_ACL_RULE_DESTINATION",
            "ARIA_UNDERLAY_ACL_RULE_DESTINATION_WILDCARD",
        )?,
        source_port_eq: optional_env("ARIA_UNDERLAY_ACL_RULE_SOURCE_PORT_EQ")
            .map(|value| value.parse::<u16>())
            .transpose()?,
        destination_port_eq: optional_env("ARIA_UNDERLAY_ACL_RULE_DESTINATION_PORT_EQ")
            .map(|value| value.parse::<u16>())
            .transpose()?,
        description: optional_env("ARIA_UNDERLAY_ACL_RULE_DESCRIPTION"),
    };

    Ok(vec![AclIntent {
        acl_id,
        kind: AclKind::AdvancedIpv4,
        name: None,
        description: optional_env("ARIA_UNDERLAY_TEST_ACL_DESCRIPTION"),
        rules: vec![rule],
    }])
}

fn desired_acl_bindings(
    member_id: &str,
    acls: &[AclIntent],
) -> Result<Vec<AclBindingIntent>, Box<dyn std::error::Error>> {
    let Some(interface_name) = optional_env("ARIA_UNDERLAY_ACL_BIND_INTERFACE") else {
        return Ok(Vec::new());
    };
    if interface_name.trim().is_empty() {
        return Ok(Vec::new());
    }
    let acl_id = match optional_env("ARIA_UNDERLAY_ACL_BIND_ID") {
        Some(value) => value.parse::<u16>()?,
        None => acls.first().map(|acl| acl.acl_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "ARIA_UNDERLAY_ACL_BIND_INTERFACE requires a declared test ACL",
            )
        })?,
    };
    Ok(vec![AclBindingIntent {
        device_id: DeviceId(member_id.into()),
        interface_name,
        direction: acl_direction(
            optional_env("ARIA_UNDERLAY_ACL_BIND_DIRECTION")
                .unwrap_or_else(|| "inbound".into())
                .as_str(),
        )?,
        acl_id,
    }])
}

fn ensure_requested_acls_are_creates(
    change_sets: &[aria_underlay::engine::diff::ChangeSet],
    acls: &[AclIntent],
) -> Result<(), Box<dyn std::error::Error>> {
    for acl in acls {
        let created = change_sets
            .iter()
            .flat_map(|change_set| &change_set.ops)
            .any(|op| matches!(op, ChangeOp::CreateAcl(created) if created.acl_id == acl.acl_id));
        if !created {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "ACL {} was not planned as CreateAcl; choose a non-existing ACL id",
                    acl.acl_id
                ),
            )
            .into());
        }
    }
    Ok(())
}

fn ensure_requested_acl_bindings_are_creates(
    change_sets: &[aria_underlay::engine::diff::ChangeSet],
    bindings: &[AclBindingIntent],
) -> Result<(), Box<dyn std::error::Error>> {
    for binding in bindings {
        let created = change_sets
            .iter()
            .flat_map(|change_set| &change_set.ops)
            .any(|op| {
                matches!(
                    op,
                    ChangeOp::CreateAclBinding(created)
                        if created.interface_name == binding.interface_name
                            && created.direction == binding.direction
                            && created.acl_id == binding.acl_id
                )
            });
        if !created {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "ACL binding {} {:?} was not planned as CreateAclBinding; choose an unbound test interface",
                    binding.interface_name, binding.direction
                ),
            )
            .into());
        }
    }
    Ok(())
}

fn desired_interfaces(
    member_id: &str,
    test_vlan: u16,
) -> Result<Vec<InterfaceIntent>, Box<dyn std::error::Error>> {
    let mut interfaces = Vec::new();
    if let Ok(name) = std::env::var("ARIA_UNDERLAY_ACCESS_INTERFACE") {
        if !name.trim().is_empty() {
            interfaces.push(InterfaceIntent {
                device_id: DeviceId(member_id.into()),
                name,
                admin_state: AdminState::Up,
                description: optional_env("ARIA_UNDERLAY_ACCESS_DESCRIPTION"),
                mode: PortMode::Access { vlan_id: test_vlan },
            });
        }
    }
    if let Ok(name) = std::env::var("ARIA_UNDERLAY_TRUNK_INTERFACE") {
        if !name.trim().is_empty() {
            let allowed_vlans = parse_vlan_list(required_env(
                "ARIA_UNDERLAY_TRUNK_ALLOWED_VLANS",
            )?)?;
            if !allowed_vlans.contains(&test_vlan) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ARIA_UNDERLAY_TRUNK_ALLOWED_VLANS must include ARIA_UNDERLAY_TEST_VLAN",
                )
                .into());
            }
            interfaces.push(InterfaceIntent {
                device_id: DeviceId(member_id.into()),
                name,
                admin_state: AdminState::Up,
                description: optional_env("ARIA_UNDERLAY_TRUNK_DESCRIPTION"),
                mode: PortMode::Trunk {
                    native_vlan: None,
                    allowed_vlans,
                },
            });
        }
    }
    Ok(interfaces)
}

fn desired_vlans(
    test_vlan: u16,
    vlan_name: String,
    vlan_description: Option<String>,
    interfaces: &[InterfaceIntent],
) -> Vec<VlanIntent> {
    let mut vlan_ids = BTreeSet::from([test_vlan]);
    for interface in interfaces {
        match &interface.mode {
            PortMode::Access { vlan_id } => {
                vlan_ids.insert(*vlan_id);
            }
            PortMode::Trunk {
                native_vlan,
                allowed_vlans,
            } => {
                if let Some(vlan_id) = native_vlan {
                    vlan_ids.insert(*vlan_id);
                }
                vlan_ids.extend(allowed_vlans.iter().copied());
            }
        }
    }
    vlan_ids
        .into_iter()
        .map(|vlan_id| VlanIntent {
            vlan_id,
            name: (vlan_id == test_vlan).then(|| vlan_name.clone()),
            description: (vlan_id == test_vlan)
                .then(|| vlan_description.clone())
                .flatten(),
        })
        .collect()
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    std::env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("missing required environment variable {name}"),
        )
        .into()
    })
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn acl_action(value: &str) -> Result<AclAction, Box<dyn std::error::Error>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "permit" => Ok(AclAction::Permit),
        "deny" => Ok(AclAction::Deny),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ACL rule action must be permit or deny",
        )
        .into()),
    }
}

fn acl_protocol(value: &str) -> Result<AclProtocol, Box<dyn std::error::Error>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ip" => Ok(AclProtocol::Ip),
        "tcp" => Ok(AclProtocol::Tcp),
        "udp" => Ok(AclProtocol::Udp),
        "icmp" => Ok(AclProtocol::Icmp),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ACL rule protocol must be ip, tcp, udp, or icmp",
        )
        .into()),
    }
}

fn acl_direction(value: &str) -> Result<AclDirection, Box<dyn std::error::Error>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inbound" | "in" => Ok(AclDirection::Inbound),
        "outbound" | "out" => Ok(AclDirection::Outbound),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported ACL binding direction {other}"),
        )
        .into()),
    }
}

fn acl_endpoint(
    address_env: &str,
    wildcard_env: &str,
) -> Result<Option<AclEndpoint>, Box<dyn std::error::Error>> {
    match (optional_env(address_env), optional_env(wildcard_env)) {
        (Some(address), Some(wildcard)) => Ok(Some(AclEndpoint { address, wildcard })),
        (None, None) => Ok(None),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{address_env} and {wildcard_env} must be set together"),
        )
        .into()),
    }
}

fn env_u16(name: &str, default: u16) -> Result<u16, Box<dyn std::error::Error>> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value.parse::<u16>()?),
        _ => Ok(default),
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, Box<dyn std::error::Error>> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => match value.as_str() {
            "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
            "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be a boolean"),
            )
            .into()),
        },
        _ => Ok(default),
    }
}

fn parse_vlan_list(value: String) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
    let mut vlans = Vec::new();
    for token in value.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        vlans.push(token.parse::<u16>()?);
    }
    if vlans.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VLAN list must not be empty",
        )
        .into());
    }
    Ok(vlans)
}
