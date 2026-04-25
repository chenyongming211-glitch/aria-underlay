use std::io;

use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInventory, HostKeyPolicy, RegisterDeviceRequest};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let device_id = std::env::var("ARIA_UNDERLAY_DEVICE_ID").unwrap_or_else(|_| "leaf-a".into());
    let management_ip = required_env("ARIA_UNDERLAY_MGMT_IP")?;
    let management_port = std::env::var("ARIA_UNDERLAY_MGMT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(830);
    let secret_ref = std::env::var("ARIA_UNDERLAY_SECRET_REF")
        .unwrap_or_else(|_| "local/real-device".into());
    let expected_strategy = std::env::var("ARIA_UNDERLAY_EXPECTED_STRATEGY")
        .ok()
        .filter(|value| !value.is_empty());

    let inventory = DeviceInventory::default();
    let service = AriaUnderlayService::new(inventory.clone());
    let device_id = DeviceId(device_id);

    service
        .register_device(RegisterDeviceRequest {
            tenant_id: std::env::var("ARIA_UNDERLAY_TENANT_ID")
                .unwrap_or_else(|_| "tenant-a".into()),
            site_id: std::env::var("ARIA_UNDERLAY_SITE_ID")
                .unwrap_or_else(|_| "site-a".into()),
            device_id: device_id.clone(),
            management_ip,
            management_port,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref,
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint,
        })
        .await?;

    let onboarding = service.onboard_device(device_id.clone()).await;
    let managed = inventory.get(&device_id)?;

    println!("onboarding_result={onboarding:?}");
    println!("lifecycle_state={:?}", managed.info.lifecycle_state);

    let capability = managed
        .capability
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "adapter returned no capability profile"))?;

    println!("vendor={:?}", capability.vendor);
    println!("model={:?}", capability.model);
    println!("os_version={:?}", capability.os_version);
    println!("supports_netconf={}", capability.supports_netconf);
    println!("supports_candidate={}", capability.supports_candidate);
    println!("supports_validate={}", capability.supports_validate);
    println!(
        "supports_confirmed_commit={}",
        capability.supports_confirmed_commit
    );
    println!("supports_persist_id={}", capability.supports_persist_id);
    println!(
        "supports_rollback_on_error={}",
        capability.supports_rollback_on_error
    );
    println!(
        "supports_writable_running={}",
        capability.supports_writable_running
    );
    println!("supported_backends={:?}", capability.supported_backends);
    println!("recommended_strategy={:?}", capability.recommended_strategy);
    println!("warnings={:?}", capability.warnings);
    println!("raw_capabilities:");
    for capability in &capability.raw_capabilities {
        println!("  - {capability}");
    }

    if let Some(expected_strategy) = expected_strategy {
        let actual = format!("{:?}", capability.recommended_strategy);
        if actual != expected_strategy {
            return Err(format!(
                "expected strategy {expected_strategy}, got {actual}"
            )
            .into());
        }
    }

    Ok(())
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
