use aria_underlay::device::{
    bootstrap::validate_switch_pair, onboarding::lifecycle_state_for_onboarding_error,
    DeviceInventory, DeviceLifecycleState, DeviceRegistrationService, HostKeyPolicy,
    InMemorySecretStore, InitializeUnderlaySiteRequest, NetconfCredentialInput,
    RegisterDeviceRequest, SecretStore, SiteInitializationStatus, SwitchBootstrapRequest,
    UnderlaySiteInitializationService,
};
use aria_underlay::intent::validation::validate_switch_pair_intent;
use aria_underlay::intent::SwitchPairIntent;
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
        errors: vec![],
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
        errors: vec![],
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

#[test]
fn secret_store_creates_device_scoped_secret_ref() {
    let store = InMemorySecretStore::default();

    let secret_ref = store
        .create_for_device(
            "tenant-a",
            "site-a",
            &DeviceId("leaf-a".into()),
            NetconfCredentialInput::Password {
                username: "netconf".into(),
                password: "secret".into(),
            },
        )
        .expect("secret creation should succeed");

    assert_eq!(secret_ref, "local/tenant-a/site-a/leaf-a");
    assert!(store.get(&secret_ref).is_some());
}

#[test]
fn switch_pair_validation_requires_two_switches() {
    let request = InitializeUnderlaySiteRequest {
        request_id: "req-a".into(),
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        adapter_endpoint: "http://127.0.0.1:50051".into(),
        switches: vec![switch_bootstrap("leaf-a", DeviceRole::LeafA)],
        allow_degraded: false,
    };

    assert!(validate_switch_pair(&request).is_err());
}

#[test]
fn switch_pair_validation_requires_leaf_a_and_leaf_b() {
    let request = InitializeUnderlaySiteRequest {
        request_id: "req-a".into(),
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        adapter_endpoint: "http://127.0.0.1:50051".into(),
        switches: vec![
            switch_bootstrap("leaf-a", DeviceRole::LeafA),
            switch_bootstrap("leaf-b", DeviceRole::LeafA),
        ],
        allow_degraded: false,
    };

    assert!(validate_switch_pair(&request).is_err());
}

#[test]
fn switch_pair_validation_accepts_leaf_pair() {
    let request = InitializeUnderlaySiteRequest {
        request_id: "req-a".into(),
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        adapter_endpoint: "http://127.0.0.1:50051".into(),
        switches: vec![
            switch_bootstrap("leaf-a", DeviceRole::LeafA),
            switch_bootstrap("leaf-b", DeviceRole::LeafB),
        ],
        allow_degraded: false,
    };

    validate_switch_pair(&request).expect("leaf pair should be valid");
}

#[test]
fn empty_intent_validation_returns_invalid_intent() {
    let intent = SwitchPairIntent {
        pair_id: "pair-1".into(),
        switches: vec![],
        vlans: vec![],
        interfaces: vec![],
    };

    let err = validate_switch_pair_intent(&intent).unwrap_err();
    assert!(matches!(err, UnderlayError::InvalidIntent(_)));
    assert!(format!("{err}").contains("no switches"));
}

#[test]
fn site_initialization_status_is_serializable() {
    let encoded = serde_json::to_string(&SiteInitializationStatus::Ready)
        .expect("status should serialize");

    assert_eq!(encoded, "\"Ready\"");
}

#[tokio::test]
async fn site_initialization_accepts_custom_secret_store() {
    let initializer =
        UnderlaySiteInitializationService::new(DeviceInventory::default(), RejectingSecretStore);
    let response = initializer
        .initialize_site(InitializeUnderlaySiteRequest {
            request_id: "req-a".into(),
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            adapter_endpoint: "http://127.0.0.1:50051".into(),
            switches: vec![
                switch_bootstrap("leaf-a", DeviceRole::LeafA),
                switch_bootstrap("leaf-b", DeviceRole::LeafB),
            ],
            allow_degraded: false,
        })
        .await
        .expect("initialization should return per-device errors");

    assert_eq!(response.status, SiteInitializationStatus::PartiallyRegistered);
    assert!(response.devices.iter().all(|device| device.error.is_some()));
}

fn switch_bootstrap(device_id: &str, role: DeviceRole) -> SwitchBootstrapRequest {
    SwitchBootstrapRequest {
        device_id: DeviceId(device_id.into()),
        role,
        management_ip: "127.0.0.1".into(),
        management_port: 830,
        vendor_hint: Some(Vendor::Unknown),
        model_hint: None,
        host_key_policy: HostKeyPolicy::TrustOnFirstUse,
        credential: NetconfCredentialInput::ExistingSecretRef {
            secret_ref: format!("local/{device_id}"),
        },
    }
}

#[derive(Debug)]
struct RejectingSecretStore;

impl SecretStore for RejectingSecretStore {
    fn create_for_device(
        &self,
        _tenant_id: &str,
        _site_id: &str,
        _device_id: &DeviceId,
        _credential: NetconfCredentialInput,
    ) -> aria_underlay::UnderlayResult<String> {
        Err(UnderlayError::InvalidDeviceState(
            "test secret store rejected credential".into(),
        ))
    }
}
