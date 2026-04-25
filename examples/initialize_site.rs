use aria_underlay::device::{
    HostKeyPolicy, InMemorySecretStore, InitializeUnderlaySiteRequest, NetconfCredentialInput,
    SwitchBootstrapRequest, UnderlaySiteInitializationService,
};
use aria_underlay::model::{DeviceId, DeviceRole, Vendor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let allow_degraded = std::env::var("ARIA_UNDERLAY_ALLOW_DEGRADED")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let expected_status = std::env::var("ARIA_UNDERLAY_EXPECTED_SITE_STATUS")
        .unwrap_or_else(|_| "Ready".into());

    let inventory = aria_underlay::device::DeviceInventory::default();
    let secret_store = InMemorySecretStore::default();
    let initializer = UnderlaySiteInitializationService::new(inventory, secret_store);

    let response = initializer
        .initialize_site(InitializeUnderlaySiteRequest {
            request_id: "init-req-a".into(),
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            adapter_endpoint,
            switches: vec![
                switch("leaf-a", DeviceRole::LeafA),
                switch("leaf-b", DeviceRole::LeafB),
            ],
            allow_degraded,
        })
        .await?;

    let actual_status = format!("{:?}", response.status);
    println!("site_status={actual_status}");
    println!("devices={:#?}", response.devices);

    if actual_status != expected_status {
        return Err(format!("expected site status {expected_status}, got {actual_status}").into());
    }

    Ok(())
}

fn switch(device_id: &str, role: DeviceRole) -> SwitchBootstrapRequest {
    SwitchBootstrapRequest {
        device_id: DeviceId(device_id.into()),
        role,
        management_ip: "127.0.0.1".into(),
        management_port: 830,
        vendor_hint: Some(Vendor::Unknown),
        model_hint: None,
        host_key_policy: HostKeyPolicy::TrustOnFirstUse,
        credential: NetconfCredentialInput::Password {
            username: "netconf".into(),
            password: "secret".into(),
        },
    }
}
