use aria_underlay::device::{
    DeviceInventory, DeviceOnboardingService, DeviceRegistrationService, HostKeyPolicy,
    RegisterDeviceRequest,
};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        adapter_endpoint: "http://127.0.0.1:50051".into(),
    })?;

    let state = onboarding.onboard_device(device_id.clone()).await?;
    let managed = inventory.get(&device_id)?;

    println!("onboarding state={state:?}");
    println!("capability={:#?}", managed.capability);

    Ok(())
}

