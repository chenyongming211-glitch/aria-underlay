use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use aria_underlay::adapter_client::mapper::AdapterOperationStatus;
use aria_underlay::adapter_client::AdapterClient;
use aria_underlay::device::{DeviceInfo, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::model::{
    AdminState, DeviceId, DeviceRole, InterfaceConfig, PortMode, Vendor, VlanConfig,
};
use aria_underlay::planner::device_plan::DeviceDesiredState;
use aria_underlay::UnderlayError;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let adapter_endpoint = std::env::var("ARIA_UNDERLAY_ADAPTER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let expected_prepare =
        std::env::var("ARIA_UNDERLAY_EXPECTED_PREPARE").unwrap_or_else(|_| "Prepared".into());

    let device = DeviceInfo {
        tenant_id: "tenant-a".into(),
        site_id: "site-a".into(),
        id: DeviceId("leaf-a".into()),
        management_ip: "127.0.0.1".into(),
        management_port: 830,
        vendor_hint: Some(Vendor::Unknown),
        model_hint: None,
        role: DeviceRole::LeafA,
        secret_ref: "local/test-device".into(),
        host_key_policy: HostKeyPolicy::TrustOnFirstUse,
        adapter_endpoint: adapter_endpoint.clone(),
        lifecycle_state: DeviceLifecycleState::Pending,
    };

    let mut client = AdapterClient::connect(adapter_endpoint).await?;
    let current = client.get_current_state(&device).await?;

    if !current.vlans.contains_key(&100) {
        return Err(io::Error::new(io::ErrorKind::Other, "mock state missing VLAN 100").into());
    }
    if !current.interfaces.contains_key("GE1/0/1") {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "mock state missing interface GE1/0/1",
        )
        .into());
    }

    let desired = desired_state(device.id.clone());
    match expected_prepare.as_str() {
        "Prepared" => {
            let outcome = client.prepare(&device, &desired).await?;
            if outcome.status != AdapterOperationStatus::Prepared {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("expected Prepared, got {:?}", outcome.status),
                )
                .into());
            }
            println!(
                "current_state_ok=true prepare_status={:?} changed={}",
                outcome.status, outcome.changed
            );
        }
        expected_error_code => match client.prepare(&device, &desired).await {
            Err(UnderlayError::AdapterOperation { code, .. }) if code == expected_error_code => {
                println!("current_state_ok=true prepare_error_code={code}");
            }
            Err(err) => return Err(err.into()),
            Ok(outcome) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "expected prepare error {expected_error_code}, got outcome {:?}",
                        outcome.status
                    ),
                )
                .into());
            }
        },
    }

    Ok(())
}

fn desired_state(device_id: DeviceId) -> DeviceDesiredState {
    let mut vlans = BTreeMap::new();
    vlans.insert(
        100,
        VlanConfig {
            vlan_id: 100,
            name: Some("tenant-100".into()),
            description: Some("mock vlan".into()),
        },
    );

    let mut interfaces = BTreeMap::new();
    interfaces.insert(
        "GE1/0/1".into(),
        InterfaceConfig {
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: Some("server uplink".into()),
            mode: PortMode::Access { vlan_id: 100 },
        },
    );

    DeviceDesiredState {
        device_id,
        vlans,
        interfaces,
    }
}
