use aria_underlay::device::{
    DeviceInventory, DeviceOnboardingService, DeviceRegistrationService, HostKeyPolicy,
    RegisterDeviceRequest,
};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let expected_state = std::env::var("ARIA_UNDERLAY_EXPECTED_STATE").ok();

    let inventory = DeviceInventory::default();
    let registration = DeviceRegistrationService::new(inventory.clone());
    let onboarding = DeviceOnboardingService::new(inventory.clone());

    let device_id = DeviceId("leaf-a".into());
    registration.register(RegisterDeviceRequest {
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        device_id: device_id.clone(),
        management_ip: "127.0.0.1".into(),
        management_port: 830,
        vendor_hint: Some(Vendor::Unknown),
        model_hint: None,
        role: DeviceRole::LeafA,
        secret_ref: "local/test-device".into(),
        host_key_policy: HostKeyPolicy::TrustOnFirstUse,
        adapter_endpoint,
    })?;

    let result = onboarding.onboard_device(device_id.clone()).await;
    let managed = inventory.get(&device_id)?;
    let actual_state = format!("{:?}", managed.info.lifecycle_state);

    println!("onboarding_result={result:?}");
    println!("lifecycle_state={actual_state}");
    println!("capability={:#?}", managed.capability);

    if let Some(expected_state) = expected_state {
        if actual_state != expected_state {
            return Err(format!(
                "expected lifecycle state {expected_state}, got {actual_state}"
            )
            .into());
        }
    } else {
        result?;
    }

    Ok(())
}

