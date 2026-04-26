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

mod common;

use common::{failed_result, observed_access_state, start_test_adapter, TestAdapter};

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

#[tokio::test]
async fn adapter_transport_failure_returns_failure_without_creating_journal() {
    let inventory = inventory_with_endpoint("stack-mgmt", DeviceLifecycleState::Ready);
    let journal = Arc::new(InMemoryTxJournalStore::default());
    let service = AriaUnderlayService::new_with_journal(inventory, journal.clone());

    let response = service
        .apply_domain_intent(apply_request(DriftPolicy::ReportOnly))
        .await
        .expect("transport failure should be returned as per-device result");

    assert_eq!(response.status, ApplyStatus::Failed);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some("ADAPTER_TRANSPORT")
    );
    assert!(!response.device_results[0].changed);
    assert!(
        journal
            .list_recoverable()
            .expect("journal list should succeed")
            .is_empty(),
        "preflight transport failure must not create a fake transaction"
    );
}

#[tokio::test]
async fn prepare_failure_rolls_back_and_records_rolled_back_phase() {
    assert_adapter_failure_records_terminal_phase(
        AdapterFailurePoint::Prepare,
        "PREPARE_FAILED",
        TxPhase::RolledBack,
    )
    .await;
}

#[tokio::test]
async fn commit_failure_rolls_back_and_records_rolled_back_phase() {
    assert_adapter_failure_records_terminal_phase(
        AdapterFailurePoint::Commit,
        "COMMIT_FAILED",
        TxPhase::RolledBack,
    )
    .await;
}

#[tokio::test]
async fn verify_failure_rolls_back_and_records_rolled_back_phase() {
    assert_adapter_failure_records_terminal_phase(
        AdapterFailurePoint::Verify,
        "VERIFY_FAILED",
        TxPhase::RolledBack,
    )
    .await;
}

async fn assert_adapter_failure_records_terminal_phase(
    failure_point: AdapterFailurePoint,
    expected_error: &str,
    expected_phase: TxPhase,
) {
    let endpoint = start_fake_adapter(failure_point).await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let journal = Arc::new(InMemoryTxJournalStore::default());
    let service = AriaUnderlayService::new_with_journal(inventory, journal.clone());

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("adapter failure should be returned as per-device result");

    assert_eq!(response.status, ApplyStatus::RolledBack);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some(expected_error)
    );
    let tx_id = response.device_results[0]
        .tx_id
        .as_deref()
        .expect("failed changed transaction should include tx_id");
    let record = journal
        .get(tx_id)
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert_eq!(record.phase, expected_phase);
    assert_eq!(record.error_code.as_deref(), Some(expected_error));
}

fn apply_request(drift_policy: DriftPolicy) -> ApplyDomainIntentRequest {
    apply_request_with_vlan(100, drift_policy)
}

fn apply_request_with_vlan(vlan_id: u16, drift_policy: DriftPolicy) -> ApplyDomainIntentRequest {
    ApplyDomainIntentRequest {
        request_id: "req-apply".into(),
        trace_id: Some("trace-apply".into()),
        intent: domain_intent(vlan_id),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
            drift_policy,
        },
    }
}

fn domain_intent(vlan_id: u16) -> UnderlayDomainIntent {
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
            vlan_id,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![InterfaceIntent {
            device_id: DeviceId("member-a".into()),
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: None,
            mode: PortMode::Access { vlan_id },
        }],
    }
}

fn inventory_with_endpoint(device_id: &str, state: DeviceLifecycleState) -> DeviceInventory {
    inventory_with_endpoint_at(device_id, state, "http://127.0.0.1:59999".into())
}

fn inventory_with_endpoint_at(
    device_id: &str,
    state: DeviceLifecycleState,
    adapter_endpoint: String,
) -> DeviceInventory {
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
            adapter_endpoint,
            lifecycle_state: state,
        })
        .expect("endpoint device should be inserted");
    inventory
}

async fn start_fake_adapter(failure_point: AdapterFailurePoint) -> String {
    let mut adapter = TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        ..Default::default()
    };
    match failure_point {
        AdapterFailurePoint::Prepare => {
            adapter.prepare_result = failed_result("PREPARE_FAILED");
        }
        AdapterFailurePoint::Commit => {
            adapter.commit_result = failed_result("COMMIT_FAILED");
        }
        AdapterFailurePoint::Verify => {
            adapter.verify_result = failed_result("VERIFY_FAILED");
        }
    }
    start_test_adapter(adapter).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterFailurePoint {
    Prepare,
    Commit,
    Verify,
}
