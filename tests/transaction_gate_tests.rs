use std::sync::Arc;

use aria_underlay::api::request::{ApplyDomainIntentRequest, ApplyOptions};
use aria_underlay::api::response::ApplyStatus;
use aria_underlay::api::AriaUnderlayService;
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{AdminState, DeviceId, DeviceRole, PortMode, Vendor};
use aria_underlay::state::drift::DriftPolicy;
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};

#[tokio::test]
async fn apply_is_blocked_before_adapter_when_endpoint_has_in_doubt_transaction() {
    let inventory = inventory_with_endpoint("stack-mgmt", DeviceLifecycleState::Ready);
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-in-doubt".into(),
                    request_id: "req-old".into(),
                    trace_id: "trace-old".into(),
                },
                vec![DeviceId("stack-mgmt".into())],
            )
            .with_phase(TxPhase::InDoubt),
        )
        .expect("in-doubt journal record should be stored");
    let service = AriaUnderlayService::new_with_journal(inventory, journal);

    let response = service
        .apply_domain_intent(apply_request(DriftPolicy::ReportOnly))
        .await
        .expect("apply should return per-device failure result");

    assert_eq!(response.status, ApplyStatus::InDoubt);
    assert_eq!(response.device_results.len(), 1);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some("TX_IN_DOUBT")
    );
    assert!(!response.device_results[0].changed);
}

#[tokio::test]
async fn block_new_transaction_policy_blocks_drifted_endpoint_before_adapter() {
    let inventory = inventory_with_endpoint("stack-mgmt", DeviceLifecycleState::Drifted);
    let service = AriaUnderlayService::new(inventory);

    let response = service
        .apply_domain_intent(apply_request(DriftPolicy::BlockNewTransaction))
        .await
        .expect("apply should return per-device drift failure result");

    assert_eq!(response.status, ApplyStatus::Failed);
    assert_eq!(response.device_results.len(), 1);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some("DRIFT_BLOCKED")
    );
    assert!(!response.device_results[0].changed);
}

fn apply_request(drift_policy: DriftPolicy) -> ApplyDomainIntentRequest {
    ApplyDomainIntentRequest {
        request_id: "req-apply".into(),
        trace_id: Some("trace-apply".into()),
        intent: domain_intent(),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
            drift_policy,
        },
    }
}

fn domain_intent() -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "domain-a".into(),
        topology: UnderlayTopology::StackSingleManagementIp,
        endpoints: vec![ManagementEndpointIntent {
            endpoint_id: "stack-mgmt".into(),
            host: "127.0.0.1".into(),
            port: 830,
            secret_ref: "local/stack-mgmt".into(),
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
        }],
        members: vec![SwitchMemberIntent {
            member_id: "member-a".into(),
            role: Some(DeviceRole::LeafA),
            management_endpoint_id: "stack-mgmt".into(),
        }],
        vlans: vec![VlanIntent {
            vlan_id: 100,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![InterfaceIntent {
            device_id: DeviceId("member-a".into()),
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: None,
            mode: PortMode::Access { vlan_id: 100 },
        }],
    }
}

fn inventory_with_endpoint(device_id: &str, state: DeviceLifecycleState) -> DeviceInventory {
    let inventory = DeviceInventory::default();
    inventory
        .insert(DeviceInfo {
            tenant_id: "tenant-a".into(),
            site_id: "site-a".into(),
            id: DeviceId(device_id.into()),
            management_ip: "127.0.0.1".into(),
            management_port: 830,
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
            role: DeviceRole::LeafA,
            secret_ref: format!("local/{device_id}"),
            host_key_policy: HostKeyPolicy::TrustOnFirstUse,
            adapter_endpoint: "http://127.0.0.1:59999".into(),
            lifecycle_state: state,
        })
        .expect("endpoint device should be inserted");
    inventory
}
