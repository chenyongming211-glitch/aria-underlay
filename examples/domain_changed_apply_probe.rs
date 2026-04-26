use aria_underlay::api::request::{ApplyDomainIntentRequest, ApplyOptions};
use aria_underlay::api::response::ApplyStatus;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInventory, HostKeyPolicy, RegisterDeviceRequest};
use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{AdminState, DeviceId, DeviceRole, PortMode, Vendor};
use aria_underlay::tx::TransactionStrategy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let inventory = DeviceInventory::default();
    let service = AriaUnderlayService::new(inventory.clone());

    register_endpoint(&service, adapter_endpoint, "stack-mgmt").await?;

    let request = ApplyDomainIntentRequest {
        request_id: "domain-changed-apply-req-a".into(),
        trace_id: Some("domain-changed-apply-trace-a".into()),
        intent: changed_stack_intent(),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
        },
    };

    let dry_run = service.dry_run_domain(request.clone()).await?;
    println!("domain_changed_dry_run_noop={}", dry_run.noop);
    println!("domain_changed_change_sets={:#?}", dry_run.change_sets);
    if dry_run.noop {
        return Err("expected changed domain dry-run noop=false".into());
    }

    let response = service.apply_domain_intent(request).await?;
    println!("domain_changed_apply_status={:?}", response.status);
    println!("domain_changed_strategy={:?}", response.strategy);
    println!("domain_changed_device_results={:#?}", response.device_results);

    if response.status != ApplyStatus::Success {
        return Err(format!("expected Success, got {:?}", response.status).into());
    }
    if response.tx_id.as_deref().unwrap_or_default().is_empty() {
        return Err("expected changed apply response to include tx_id".into());
    }
    if response.strategy != Some(TransactionStrategy::ConfirmedCommit) {
        return Err(format!(
            "expected ConfirmedCommit strategy, got {:?}",
            response.strategy
        )
        .into());
    }
    if response.device_results.len() != 1 {
        return Err(format!(
            "expected one management endpoint result, got {}",
            response.device_results.len()
        )
        .into());
    }
    if !response.device_results.iter().any(|result| result.changed) {
        return Err("expected at least one endpoint result changed=true".into());
    }

    Ok(())
}

async fn register_endpoint(
    service: &AriaUnderlayService,
    adapter_endpoint: String,
    endpoint_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    service
        .register_device(RegisterDeviceRequest {
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            device_id: DeviceId(endpoint_id.into()),
            management_ip: "127.0.0.1".into(),
            management_port: 830,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref: format!("local/{endpoint_id}"),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint,
        })
        .await?;
    Ok(())
}

fn changed_stack_intent() -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "stack-domain-a".into(),
        topology: UnderlayTopology::StackSingleManagementIp,
        endpoints: vec![ManagementEndpointIntent {
            endpoint_id: "stack-mgmt".into(),
            host: "127.0.0.1".into(),
            port: 830,
            secret_ref: "local/stack-mgmt".into(),
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
        }],
        members: vec![
            SwitchMemberIntent {
                member_id: "member-a".into(),
                role: Some(DeviceRole::LeafA),
                management_endpoint_id: "stack-mgmt".into(),
            },
            SwitchMemberIntent {
                member_id: "member-b".into(),
                role: Some(DeviceRole::LeafB),
                management_endpoint_id: "stack-mgmt".into(),
            },
        ],
        vlans: vec![VlanIntent {
            vlan_id: 100,
            name: Some("tenant-100".into()),
            description: Some("mock vlan".into()),
        }],
        interfaces: vec![InterfaceIntent {
            device_id: DeviceId("member-a".into()),
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: Some("server uplink".into()),
            mode: PortMode::Access { vlan_id: 100 },
        }],
    }
}
