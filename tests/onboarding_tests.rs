use aria_underlay::device::{
    DeviceInventory, DeviceRegistrationService, DeviceLifecycleState, HostKeyPolicy,
    RegisterDeviceRequest,
};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};

#[test]
fn register_device_starts_pending() {
    let inventory = DeviceInventory::default();
    let registration = DeviceRegistrationService::new(inventory);

    let response = registration
        .register(RegisterDeviceRequest {
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
        })
        .expect("device registration should succeed");

    assert_eq!(response.lifecycle_state, DeviceLifecycleState::Pending);
}

