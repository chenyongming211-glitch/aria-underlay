use aria_underlay::api::request::{ApplyIntentRequest, ApplyOptions};
use aria_underlay::api::response::ApplyStatus;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInventory, HostKeyPolicy, RegisterDeviceRequest};
use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{SwitchIntent, SwitchPairIntent};
use aria_underlay::model::{AdminState, DeviceId, DeviceRole, PortMode, Vendor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let inventory = DeviceInventory::default();
    let service = AriaUnderlayService::new(inventory.clone());

    register_switch(
        &service,
        adapter_endpoint.clone(),
        "leaf-a",
        DeviceRole::LeafA,
    )
    .await?;
    register_switch(&service, adapter_endpoint, "leaf-b", DeviceRole::LeafB).await?;

    let request = ApplyIntentRequest {
        request_id: "noop-apply-req-a".into(),
        trace_id: Some("noop-apply-trace-a".into()),
        intent: mock_current_state_intent(),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
        },
    };

    let dry_run = service.dry_run(request.clone()).await?;
    println!("dry_run_noop={}", dry_run.noop);
    println!("dry_run_change_sets={:#?}", dry_run.change_sets);
    if !dry_run.noop {
        return Err("expected dry-run noop=true".into());
    }

    let response = service.apply_intent(request).await?;
    println!("apply_status={:?}", response.status);
    println!("device_results={:#?}", response.device_results);

    if response.status != ApplyStatus::NoOpSuccess {
        return Err(format!("expected NoOpSuccess, got {:?}", response.status).into());
    }
    if response.device_results.iter().any(|result| result.changed) {
        return Err("expected every device result changed=false".into());
    }

    Ok(())
}

async fn register_switch(
    service: &AriaUnderlayService,
    adapter_endpoint: String,
    device_id: &str,
    role: DeviceRole,
) -> Result<(), Box<dyn std::error::Error>> {
    service
        .register_device(RegisterDeviceRequest {
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            device_id: DeviceId(device_id.into()),
            management_ip: "127.0.0.1".into(),
            management_port: 830,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role,
            secret_ref: format!("local/{device_id}"),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint,
        })
        .await?;
    Ok(())
}

fn mock_current_state_intent() -> SwitchPairIntent {
    SwitchPairIntent {
        pair_id: "pair-a".into(),
        switches: vec![
            SwitchIntent {
                device_id: DeviceId("leaf-a".into()),
                role: DeviceRole::LeafA,
            },
            SwitchIntent {
                device_id: DeviceId("leaf-b".into()),
                role: DeviceRole::LeafB,
            },
        ],
        vlans: vec![VlanIntent {
            vlan_id: 100,
            name: Some("prod".into()),
            description: Some("production vlan".into()),
        }],
        interfaces: vec![access_interface("leaf-a"), access_interface("leaf-b")],
    }
}

fn access_interface(device_id: &str) -> InterfaceIntent {
    InterfaceIntent {
        device_id: DeviceId(device_id.into()),
        name: "GE1/0/1".into(),
        admin_state: AdminState::Up,
        description: Some("server uplink".into()),
        mode: PortMode::Access { vlan_id: 100 },
    }
}
