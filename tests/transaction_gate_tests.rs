use std::sync::Arc;

use async_trait::async_trait;
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
use aria_underlay::proto::adapter;
use aria_underlay::proto::adapter::underlay_adapter_server::{
    UnderlayAdapter, UnderlayAdapterServer,
};
use aria_underlay::state::drift::DriftPolicy;
use aria_underlay::tx::{
    InMemoryTxJournalStore, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use tonic::{Request, Response, Status};

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
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("test adapter listener should bind");
    let addr = listener.local_addr().expect("test adapter addr should exist");
    drop(listener);
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(UnderlayAdapterServer::new(JournalPhaseFakeAdapter {
                failure_point,
            }))
            .serve(addr)
            .await
            .expect("test adapter server should run");
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterFailurePoint {
    Prepare,
    Commit,
    Verify,
}

#[derive(Debug)]
struct JournalPhaseFakeAdapter {
    failure_point: AdapterFailurePoint,
}

#[async_trait]
impl UnderlayAdapter for JournalPhaseFakeAdapter {
    async fn get_capabilities(
        &self,
        _request: Request<adapter::GetCapabilitiesRequest>,
    ) -> Result<Response<adapter::GetCapabilitiesResponse>, Status> {
        Ok(Response::new(adapter::GetCapabilitiesResponse {
            capability: Some(adapter::DeviceCapability {
                vendor: adapter::Vendor::Unknown as i32,
                model: "journal-phase-fake".into(),
                os_version: "test".into(),
                raw_capabilities: Vec::new(),
                supports_netconf: true,
                supports_candidate: true,
                supports_validate: true,
                supports_confirmed_commit: true,
                supports_persist_id: true,
                supports_rollback_on_error: false,
                supports_writable_running: false,
                supported_backends: vec![adapter::BackendKind::Netconf as i32],
            }),
            warnings: Vec::new(),
            errors: Vec::new(),
        }))
    }

    async fn get_current_state(
        &self,
        _request: Request<adapter::GetCurrentStateRequest>,
    ) -> Result<Response<adapter::GetCurrentStateResponse>, Status> {
        Ok(Response::new(adapter::GetCurrentStateResponse {
            state: Some(observed_state(100)),
            warnings: Vec::new(),
            errors: Vec::new(),
        }))
    }

    async fn dry_run(
        &self,
        _request: Request<adapter::DryRunRequest>,
    ) -> Result<Response<adapter::DryRunResponse>, Status> {
        Ok(Response::new(adapter::DryRunResponse {
            result: Some(result(adapter::AdapterOperationStatus::NoChange)),
        }))
    }

    async fn prepare(
        &self,
        _request: Request<adapter::PrepareRequest>,
    ) -> Result<Response<adapter::PrepareResponse>, Status> {
        Ok(Response::new(adapter::PrepareResponse {
            result: Some(if self.failure_point == AdapterFailurePoint::Prepare {
                failed_result("PREPARE_FAILED")
            } else {
                result(adapter::AdapterOperationStatus::Prepared)
            }),
        }))
    }

    async fn commit(
        &self,
        _request: Request<adapter::CommitRequest>,
    ) -> Result<Response<adapter::CommitResponse>, Status> {
        Ok(Response::new(adapter::CommitResponse {
            result: Some(if self.failure_point == AdapterFailurePoint::Commit {
                failed_result("COMMIT_FAILED")
            } else {
                result(adapter::AdapterOperationStatus::ConfirmedCommitPending)
            }),
        }))
    }

    async fn final_confirm(
        &self,
        _request: Request<adapter::FinalConfirmRequest>,
    ) -> Result<Response<adapter::FinalConfirmResponse>, Status> {
        Ok(Response::new(adapter::FinalConfirmResponse {
            result: Some(result(adapter::AdapterOperationStatus::Committed)),
        }))
    }

    async fn rollback(
        &self,
        _request: Request<adapter::RollbackRequest>,
    ) -> Result<Response<adapter::RollbackResponse>, Status> {
        Ok(Response::new(adapter::RollbackResponse {
            result: Some(result(adapter::AdapterOperationStatus::RolledBack)),
        }))
    }

    async fn verify(
        &self,
        _request: Request<adapter::VerifyRequest>,
    ) -> Result<Response<adapter::VerifyResponse>, Status> {
        Ok(Response::new(adapter::VerifyResponse {
            result: Some(if self.failure_point == AdapterFailurePoint::Verify {
                failed_result("VERIFY_FAILED")
            } else {
                result(adapter::AdapterOperationStatus::Committed)
            }),
        }))
    }

    async fn recover(
        &self,
        _request: Request<adapter::RecoverRequest>,
    ) -> Result<Response<adapter::RecoverResponse>, Status> {
        Ok(Response::new(adapter::RecoverResponse {
            result: Some(result(adapter::AdapterOperationStatus::NoChange)),
        }))
    }

    async fn force_unlock(
        &self,
        _request: Request<adapter::ForceUnlockRequest>,
    ) -> Result<Response<adapter::ForceUnlockResponse>, Status> {
        Ok(Response::new(adapter::ForceUnlockResponse {
            result: Some(result(adapter::AdapterOperationStatus::Committed)),
        }))
    }
}

fn result(status: adapter::AdapterOperationStatus) -> adapter::AdapterResult {
    adapter::AdapterResult {
        status: status as i32,
        changed: true,
        warnings: Vec::new(),
        errors: Vec::new(),
        rollback_artifact: None,
        normalized_state: None,
    }
}

fn failed_result(code: &str) -> adapter::AdapterResult {
    adapter::AdapterResult {
        status: adapter::AdapterOperationStatus::Failed as i32,
        changed: false,
        warnings: Vec::new(),
        errors: vec![adapter::AdapterError {
            code: code.into(),
            message: format!("{code} for test adapter"),
            normalized_error: code.into(),
            raw_error_summary: code.into(),
            retryable: false,
        }],
        rollback_artifact: None,
        normalized_state: None,
    }
}

fn observed_state(vlan_id: u32) -> adapter::ObservedDeviceState {
    adapter::ObservedDeviceState {
        device_id: "stack-mgmt".into(),
        vlans: vec![adapter::VlanConfig {
            vlan_id,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![adapter::InterfaceConfig {
            name: "GE1/0/1".into(),
            admin_state: adapter::AdminState::Up as i32,
            description: None,
            mode: Some(adapter::PortMode {
                kind: adapter::PortModeKind::Access as i32,
                access_vlan: Some(vlan_id),
                native_vlan: None,
                allowed_vlans: Vec::new(),
            }),
        }],
    }
}
