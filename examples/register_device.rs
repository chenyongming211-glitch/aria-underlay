use aria_underlay::device::{
    DeviceInventory, DeviceRegistrationService, HostKeyPolicy, RegisterDeviceRequest,
};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inventory = DeviceInventory::default();
    let registration = DeviceRegistrationService::new(inventory.clone());

    let response = registration.register(RegisterDeviceRequest {
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        device_id: DeviceId("leaf-a".into()),
        management_ip: "127.0.0.1".into(),
        management_port: 830,
        vendor_hint: Some(Vendor::Unknown),
        model_hint: None,
        role: DeviceRole::LeafA,
        secret_ref: "local/test-device".into(),
        host_key_policy: HostKeyPolicy::TrustOnFirstUse,
        adapter_endpoint: "http://127.0.0.1:50051".into(),
    })?;

    println!(
        "registered device={} state={:?}",
        response.device_id.0, response.lifecycle_state
    );

    Ok(())
}

