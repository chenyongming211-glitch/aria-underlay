use aria_underlay::device::{
    onboarding::lifecycle_state_for_onboarding_error, DeviceInventory, DeviceLifecycleState,
    DeviceRegistrationService, HostKeyPolicy, RegisterDeviceRequest,
};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};
use aria_underlay::UnderlayError;

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

#[test]
fn auth_failed_adapter_error_maps_to_auth_failed_state() {
    let error = UnderlayError::AdapterOperation {
        code: "AUTH_FAILED".into(),
        message: "bad credentials".into(),
        retryable: false,
    };

    assert_eq!(
        lifecycle_state_for_onboarding_error(&error),
        DeviceLifecycleState::AuthFailed
    );
}

#[test]
fn unreachable_adapter_error_maps_to_unreachable_state() {
    let error = UnderlayError::AdapterOperation {
        code: "DEVICE_UNREACHABLE".into(),
        message: "timeout".into(),
        retryable: true,
    };

    assert_eq!(
        lifecycle_state_for_onboarding_error(&error),
        DeviceLifecycleState::Unreachable
    );
}

#[test]
fn adapter_transport_error_maps_to_unreachable_state() {
    let error = UnderlayError::AdapterTransport("connection refused".into());

    assert_eq!(
        lifecycle_state_for_onboarding_error(&error),
        DeviceLifecycleState::Unreachable
    );
}
