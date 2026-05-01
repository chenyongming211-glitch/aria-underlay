use std::process::{Command, Output};
use std::sync::Arc;

use async_trait::async_trait;
use aria_underlay::api::request::{ApplyDomainIntentRequest, ApplyOptions};
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{
    DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy,
};
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
use aria_underlay::state::{JsonFileShadowStateStore, ShadowStateStore};
use aria_underlay::tx::{JsonFileTxJournalStore, TxJournalStore, TxPhase};
use tonic::{Request, Response, Status};

mod common;

use common::{
    adapter_result, confirmed_commit_capability, observed_access_state, start_test_adapter_at,
    TestAdapter,
};

const CHILD_MODE_ENV: &str = "ARIA_UNDERLAY_PROCESS_CHAOS_CHILD";
const JOURNAL_ROOT_ENV: &str = "ARIA_UNDERLAY_PROCESS_CHAOS_JOURNAL_ROOT";
const SHADOW_ROOT_ENV: &str = "ARIA_UNDERLAY_PROCESS_CHAOS_SHADOW_ROOT";
const ADAPTER_ENDPOINT_ENV: &str = "ARIA_UNDERLAY_PROCESS_CHAOS_ADAPTER_ENDPOINT";
const CHILD_CRASH_EXIT_CODE: i32 = 77;

#[tokio::test]
async fn process_restart_recovers_final_confirming_tx_and_persists_shadow_before_committed_journal()
{
    let temp = temp_store_dir("final-confirm-recover");
    let journal_root = temp.join("journal");
    let shadow_root = temp.join("shadow");
    let endpoint = reserve_local_endpoint();

    let output = run_crashing_child(&journal_root, &shadow_root, &endpoint);
    assert_child_crashed_at_final_confirm(output);

    let journal = Arc::new(JsonFileTxJournalStore::new(&journal_root));
    let pending_before = journal
        .list_recoverable()
        .expect("journal scan after child crash should succeed");
    assert_eq!(pending_before.len(), 1);
    assert_eq!(pending_before[0].phase, TxPhase::FinalConfirming);
    let tx_id = pending_before[0].tx_id.clone();

    let shadow = Arc::new(JsonFileShadowStateStore::new(&shadow_root));
    assert!(
        shadow
            .get(&DeviceId("stack-mgmt".into()))
            .expect("shadow read after child crash should succeed")
            .is_none(),
        "child crash should happen before normal shadow persistence"
    );

    let addr = parse_endpoint_addr(&endpoint);
    start_test_adapter_at(
        TestAdapter {
            current_state: Some(observed_access_state("stack-mgmt", 200)),
            ..Default::default()
        },
        addr,
    )
    .await;

    let service = AriaUnderlayService::new_with_shadow_store(
        inventory_with_endpoint_at(
            "stack-mgmt",
            DeviceLifecycleState::Ready,
            endpoint.clone(),
        ),
        journal.clone(),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        shadow.clone(),
    );

    let report = service
        .recover_pending_transactions()
        .await
        .expect("recovery should roll forward the final-confirming transaction");

    assert_eq!(report.recovered, 1);
    assert_eq!(report.pending, 0);
    assert_eq!(report.in_doubt, 0);

    let record = journal
        .get(&tx_id)
        .expect("journal read after recovery should succeed")
        .expect("journal record should remain readable");
    assert_eq!(record.phase, TxPhase::Committed);

    let recovered_shadow = shadow
        .get(&DeviceId("stack-mgmt".into()))
        .expect("shadow read after recovery should succeed")
        .expect("recovery must persist desired shadow before terminal journal");
    assert_eq!(recovered_shadow.revision, 1);
    assert!(recovered_shadow.vlans.contains_key(&200));
    assert_eq!(
        recovered_shadow.interfaces["GE1/0/1"].mode,
        PortMode::Access { vlan_id: 200 }
    );

    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn process_restart_marks_final_confirming_tx_in_doubt_when_adapter_session_stays_down() {
    let temp = temp_store_dir("final-confirm-session-drop");
    let journal_root = temp.join("journal");
    let shadow_root = temp.join("shadow");
    let endpoint = reserve_local_endpoint();

    let output = run_crashing_child(&journal_root, &shadow_root, &endpoint);
    assert_child_crashed_at_final_confirm(output);

    let journal = Arc::new(JsonFileTxJournalStore::new(&journal_root));
    let shadow = Arc::new(JsonFileShadowStateStore::new(&shadow_root));
    let service = AriaUnderlayService::new_with_shadow_store(
        inventory_with_endpoint_at(
            "stack-mgmt",
            DeviceLifecycleState::Ready,
            endpoint.clone(),
        ),
        journal.clone(),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        shadow,
    );

    let report = service
        .recover_pending_transactions()
        .await
        .expect("recovery scan should complete even when adapter is still down");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.pending, 1);
    assert_eq!(report.in_doubt, 1);
    assert_eq!(report.tx_ids.len(), 1);

    let record = journal
        .get(&report.tx_ids[0])
        .expect("journal read after failed recovery should succeed")
        .expect("journal record should remain readable");
    assert_eq!(record.phase, TxPhase::InDoubt);
    assert_eq!(
        record.error_code.as_deref(),
        Some("FINAL_CONFIRM_RECOVERY_IN_DOUBT")
    );
    assert!(
        record
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("could not prove final-confirming transaction")
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn process_chaos_child_crashes_during_final_confirm() {
    if std::env::var(CHILD_MODE_ENV).as_deref() != Ok("final-confirm") {
        return;
    }

    let journal_root = std::env::var_os(JOURNAL_ROOT_ENV)
        .map(std::path::PathBuf::from)
        .expect("child process requires journal root env");
    let shadow_root = std::env::var_os(SHADOW_ROOT_ENV)
        .map(std::path::PathBuf::from)
        .expect("child process requires shadow root env");
    let endpoint =
        std::env::var(ADAPTER_ENDPOINT_ENV).expect("child process requires adapter endpoint env");

    let runtime = tokio::runtime::Runtime::new().expect("child tokio runtime should start");
    runtime.block_on(async move {
        start_final_confirm_crash_adapter_at(parse_endpoint_addr(&endpoint)).await;
        let service = AriaUnderlayService::new_with_shadow_store(
            inventory_with_endpoint_at("stack-mgmt", DeviceLifecycleState::Ready, endpoint),
            Arc::new(JsonFileTxJournalStore::new(&journal_root)),
            Default::default(),
            Default::default(),
            Arc::new(aria_underlay::device::InMemorySecretStore::default()),
            Arc::new(JsonFileShadowStateStore::new(&shadow_root)),
        );

        service
            .apply_domain_intent(apply_request_with_vlan(200))
            .await
            .expect("child apply should exit during final confirm before returning");
    });

    panic!("child process should have exited from final_confirm handler");
}

fn run_crashing_child(
    journal_root: &std::path::Path,
    shadow_root: &std::path::Path,
    endpoint: &str,
) -> Output {
    Command::new(std::env::current_exe().expect("current test executable should be available"))
        .arg("--exact")
        .arg("process_chaos_child_crashes_during_final_confirm")
        .arg("--nocapture")
        .env(CHILD_MODE_ENV, "final-confirm")
        .env(JOURNAL_ROOT_ENV, journal_root)
        .env(SHADOW_ROOT_ENV, shadow_root)
        .env(ADAPTER_ENDPOINT_ENV, endpoint)
        .output()
        .expect("child process should launch")
}

fn assert_child_crashed_at_final_confirm(output: Output) {
    assert_eq!(
        output.status.code(),
        Some(CHILD_CRASH_EXIT_CODE),
        "child process should exit from final_confirm handler; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn start_final_confirm_crash_adapter_at(addr: std::net::SocketAddr) -> String {
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(UnderlayAdapterServer::new(FinalConfirmCrashAdapter))
            .serve(addr)
            .await
            .expect("crash adapter server should run until process exits");
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

fn reserve_local_endpoint() -> String {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
    let addr = listener.local_addr().expect("test listener addr should exist");
    drop(listener);
    format!("http://{addr}")
}

fn parse_endpoint_addr(endpoint: &str) -> std::net::SocketAddr {
    endpoint
        .strip_prefix("http://")
        .unwrap_or(endpoint)
        .parse()
        .expect("adapter endpoint should contain a socket address")
}

fn apply_request_with_vlan(vlan_id: u16) -> ApplyDomainIntentRequest {
    ApplyDomainIntentRequest {
        request_id: "req-process-chaos".into(),
        trace_id: Some("trace-process-chaos".into()),
        intent: domain_intent(vlan_id),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
            drift_policy: DriftPolicy::ReportOnly,
        },
    }
}

fn domain_intent(vlan_id: u16) -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "domain-process-chaos".into(),
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

fn temp_store_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-process-chaos-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}

#[derive(Debug, Clone)]
struct FinalConfirmCrashAdapter;

#[async_trait]
impl UnderlayAdapter for FinalConfirmCrashAdapter {
    async fn get_capabilities(
        &self,
        _request: Request<adapter::GetCapabilitiesRequest>,
    ) -> Result<Response<adapter::GetCapabilitiesResponse>, Status> {
        Ok(Response::new(adapter::GetCapabilitiesResponse {
            capability: Some(confirmed_commit_capability()),
            warnings: Vec::new(),
            errors: Vec::new(),
        }))
    }

    async fn get_current_state(
        &self,
        _request: Request<adapter::GetCurrentStateRequest>,
    ) -> Result<Response<adapter::GetCurrentStateResponse>, Status> {
        Ok(Response::new(adapter::GetCurrentStateResponse {
            state: Some(observed_access_state("stack-mgmt", 100)),
            warnings: Vec::new(),
            errors: Vec::new(),
        }))
    }

    async fn dry_run(
        &self,
        _request: Request<adapter::DryRunRequest>,
    ) -> Result<Response<adapter::DryRunResponse>, Status> {
        Ok(Response::new(adapter::DryRunResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::NoChange)),
        }))
    }

    async fn prepare(
        &self,
        _request: Request<adapter::PrepareRequest>,
    ) -> Result<Response<adapter::PrepareResponse>, Status> {
        Ok(Response::new(adapter::PrepareResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::Prepared)),
        }))
    }

    async fn commit(
        &self,
        _request: Request<adapter::CommitRequest>,
    ) -> Result<Response<adapter::CommitResponse>, Status> {
        Ok(Response::new(adapter::CommitResponse {
            result: Some(adapter_result(
                adapter::AdapterOperationStatus::ConfirmedCommitPending,
            )),
        }))
    }

    async fn final_confirm(
        &self,
        _request: Request<adapter::FinalConfirmRequest>,
    ) -> Result<Response<adapter::FinalConfirmResponse>, Status> {
        std::process::exit(CHILD_CRASH_EXIT_CODE);
    }

    async fn rollback(
        &self,
        _request: Request<adapter::RollbackRequest>,
    ) -> Result<Response<adapter::RollbackResponse>, Status> {
        Ok(Response::new(adapter::RollbackResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::RolledBack)),
        }))
    }

    async fn verify(
        &self,
        _request: Request<adapter::VerifyRequest>,
    ) -> Result<Response<adapter::VerifyResponse>, Status> {
        Ok(Response::new(adapter::VerifyResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::Committed)),
        }))
    }

    async fn recover(
        &self,
        _request: Request<adapter::RecoverRequest>,
    ) -> Result<Response<adapter::RecoverResponse>, Status> {
        Ok(Response::new(adapter::RecoverResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::Committed)),
        }))
    }

    async fn force_unlock(
        &self,
        _request: Request<adapter::ForceUnlockRequest>,
    ) -> Result<Response<adapter::ForceUnlockResponse>, Status> {
        Ok(Response::new(adapter::ForceUnlockResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::Committed)),
        }))
    }
}
