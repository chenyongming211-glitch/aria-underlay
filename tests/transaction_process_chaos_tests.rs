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
use aria_underlay::planner::domain_plan::plan_underlay_domain;
use aria_underlay::proto::adapter;
use aria_underlay::proto::adapter::underlay_adapter_server::{
    UnderlayAdapter, UnderlayAdapterServer,
};
use aria_underlay::state::drift::DriftPolicy;
use aria_underlay::state::{JsonFileShadowStateStore, ShadowStateStore};
use aria_underlay::tx::{
    JsonFileTxJournalStore, TransactionStrategy, TxContext, TxJournalRecord, TxJournalStore,
    TxPhase,
};
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

    let output = run_crashing_child(
        &journal_root,
        &shadow_root,
        &endpoint,
        CrashPhase::FinalConfirming,
    );
    assert_child_crashed(output, CrashPhase::FinalConfirming);

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

    let output = run_crashing_child(
        &journal_root,
        &shadow_root,
        &endpoint,
        CrashPhase::FinalConfirming,
    );
    assert_child_crashed(output, CrashPhase::FinalConfirming);

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

#[tokio::test]
async fn process_restart_rolls_back_preparing_tx_without_persisting_shadow() {
    let temp = temp_store_dir("preparing-recover");
    let journal_root = temp.join("journal");
    let shadow_root = temp.join("shadow");
    let endpoint = reserve_local_endpoint();

    let output = run_crashing_child(
        &journal_root,
        &shadow_root,
        &endpoint,
        CrashPhase::Preparing,
    );
    assert_child_crashed(output, CrashPhase::Preparing);

    let journal = Arc::new(JsonFileTxJournalStore::new(&journal_root));
    let pending_before = journal
        .list_recoverable()
        .expect("journal scan after preparing crash should succeed");
    assert_eq!(pending_before.len(), 1);
    assert_eq!(pending_before[0].phase, TxPhase::Preparing);
    let tx_id = pending_before[0].tx_id.clone();

    let shadow = Arc::new(JsonFileShadowStateStore::new(&shadow_root));
    assert!(
        shadow
            .get(&DeviceId("stack-mgmt".into()))
            .expect("shadow read after preparing crash should succeed")
            .is_none(),
        "preparing crash must not persist desired shadow"
    );

    start_test_adapter_at(
        TestAdapter {
            current_state: Some(observed_access_state("stack-mgmt", 100)),
            ..Default::default()
        },
        parse_endpoint_addr(&endpoint),
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
        .expect("preparing recovery should discard uncommitted candidate work");

    assert_eq!(report.recovered, 1);
    assert_eq!(report.pending, 0);
    assert_eq!(report.in_doubt, 0);

    let record = journal
        .get(&tx_id)
        .expect("journal read after preparing recovery should succeed")
        .expect("journal record should remain readable");
    assert_eq!(record.phase, TxPhase::RolledBack);
    assert!(
        shadow
            .get(&DeviceId("stack-mgmt".into()))
            .expect("shadow read after preparing recovery should succeed")
            .is_none(),
        "discarding a preparing transaction must not create desired shadow"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn process_restart_marks_committing_tx_in_doubt_when_adapter_session_stays_down() {
    let temp = temp_store_dir("committing-session-drop");
    let journal_root = temp.join("journal");
    let shadow_root = temp.join("shadow");
    let endpoint = reserve_local_endpoint();

    let output = run_crashing_child(
        &journal_root,
        &shadow_root,
        &endpoint,
        CrashPhase::Committing,
    );
    assert_child_crashed(output, CrashPhase::Committing);

    let journal = Arc::new(JsonFileTxJournalStore::new(&journal_root));
    let pending_before = journal
        .list_recoverable()
        .expect("journal scan after committing crash should succeed");
    assert_eq!(pending_before.len(), 1);
    assert_eq!(pending_before[0].phase, TxPhase::Committing);

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
        shadow.clone(),
    );

    let report = service
        .recover_pending_transactions()
        .await
        .expect("committing recovery scan should complete when adapter is down");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.pending, 1);
    assert_eq!(report.in_doubt, 1);

    let record = journal
        .get(&report.tx_ids[0])
        .expect("journal read after failed committing recovery should succeed")
        .expect("journal record should remain readable");
    assert_eq!(record.phase, TxPhase::InDoubt);
    assert_eq!(record.error_code.as_deref(), Some("ADAPTER_TRANSPORT"));
    assert!(
        shadow
            .get(&DeviceId("stack-mgmt".into()))
            .expect("shadow read after failed committing recovery should succeed")
            .is_none(),
        "in-doubt committing recovery must not persist desired shadow"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn process_restart_recovers_committing_tx_and_persists_shadow_when_adapter_reports_committed() {
    let temp = temp_store_dir("committing-recover");
    let journal_root = temp.join("journal");
    let shadow_root = temp.join("shadow");
    let endpoint = reserve_local_endpoint();

    let output = run_crashing_child(
        &journal_root,
        &shadow_root,
        &endpoint,
        CrashPhase::Committing,
    );
    assert_child_crashed(output, CrashPhase::Committing);

    let journal = Arc::new(JsonFileTxJournalStore::new(&journal_root));
    let pending_before = journal
        .list_recoverable()
        .expect("journal scan after committing crash should succeed");
    assert_eq!(pending_before.len(), 1);
    assert_eq!(pending_before[0].phase, TxPhase::Committing);
    let tx_id = pending_before[0].tx_id.clone();

    let shadow = Arc::new(JsonFileShadowStateStore::new(&shadow_root));
    start_test_adapter_at(
        TestAdapter {
            current_state: Some(observed_access_state("stack-mgmt", 200)),
            recover_result: adapter_result(adapter::AdapterOperationStatus::Committed),
            ..Default::default()
        },
        parse_endpoint_addr(&endpoint),
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
        .expect("committing recovery should roll forward when adapter proves commit");

    assert_eq!(report.recovered, 1);
    assert_eq!(report.pending, 0);
    assert_eq!(report.in_doubt, 0);

    let record = journal
        .get(&tx_id)
        .expect("journal read after committing recovery should succeed")
        .expect("journal record should remain readable");
    assert_eq!(record.phase, TxPhase::Committed);

    let recovered_shadow = shadow
        .get(&DeviceId("stack-mgmt".into()))
        .expect("shadow read after committing recovery should succeed")
        .expect("committing recovery must persist desired shadow before terminal journal");
    assert_eq!(recovered_shadow.revision, 1);
    assert!(recovered_shadow.vlans.contains_key(&200));
    assert_eq!(
        recovered_shadow.interfaces["GE1/0/1"].mode,
        PortMode::Access { vlan_id: 200 }
    );

    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn multi_device_committing_recovery_marks_record_in_doubt_on_mixed_adapter_outcomes() {
    let temp = temp_store_dir("multi-device-committing");
    let journal_root = temp.join("journal");
    let shadow_root = temp.join("shadow");

    let endpoint_a_addr = parse_endpoint_addr(&reserve_local_endpoint());
    let endpoint_b_addr = parse_endpoint_addr(&reserve_local_endpoint());
    let endpoint_a = start_test_adapter_at(
        TestAdapter {
            recover_result: adapter_result(adapter::AdapterOperationStatus::Committed),
            ..Default::default()
        },
        endpoint_a_addr,
    )
    .await;
    let endpoint_b = start_test_adapter_at(
        TestAdapter {
            recover_result: adapter_result(adapter::AdapterOperationStatus::RolledBack),
            ..Default::default()
        },
        endpoint_b_addr,
    )
    .await;

    let journal = Arc::new(JsonFileTxJournalStore::new(&journal_root));
    let desired_states = plan_underlay_domain(&multi_endpoint_domain_intent(300))
        .expect("multi-endpoint domain intent should plan");
    let context = TxContext {
        tx_id: "tx-multi-committing".into(),
        request_id: "req-multi-committing".into(),
        trace_id: "trace-multi-committing".into(),
    };
    journal
        .put(
            &TxJournalRecord::started(
                &context,
                vec![DeviceId("leaf-a".into()), DeviceId("leaf-b".into())],
            )
            .with_desired_states(desired_states)
            .with_strategy(TransactionStrategy::ConfirmedCommit)
            .with_phase(TxPhase::Committing),
        )
        .expect("multi-device committing journal record should be stored");

    let shadow = Arc::new(JsonFileShadowStateStore::new(&shadow_root));
    let service = AriaUnderlayService::new_with_shadow_store(
        inventory_with_two_endpoints(endpoint_a, endpoint_b),
        journal.clone(),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        shadow.clone(),
    );

    let report = service
        .recover_pending_transactions()
        .await
        .expect("multi-device recovery scan should complete");

    assert_eq!(report.recovered, 0);
    assert_eq!(report.pending, 1);
    assert_eq!(report.in_doubt, 1);
    assert_eq!(report.tx_ids, vec!["tx-multi-committing".to_string()]);

    let record = journal
        .get("tx-multi-committing")
        .expect("journal read after mixed recovery should succeed")
        .expect("journal record should remain readable");
    assert_eq!(record.phase, TxPhase::InDoubt);
    assert_eq!(record.devices.len(), 2);
    assert!(
        shadow
            .list()
            .expect("shadow list after mixed recovery should succeed")
            .is_empty(),
        "mixed recovery outcomes must not persist desired shadow"
    );

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn process_chaos_child_crashes_during_requested_phase() {
    let Ok(child_mode) = std::env::var(CHILD_MODE_ENV) else {
        return;
    };
    let crash_phase = CrashPhase::from_env(&child_mode)
        .expect("child process requires a valid crash phase env");

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
        start_phase_crash_adapter_at(parse_endpoint_addr(&endpoint), crash_phase).await;
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
            .expect("child apply should exit during requested adapter phase before returning");
    });

    panic!("child process should have exited from requested adapter handler");
}

fn run_crashing_child(
    journal_root: &std::path::Path,
    shadow_root: &std::path::Path,
    endpoint: &str,
    crash_phase: CrashPhase,
) -> Output {
    Command::new(std::env::current_exe().expect("current test executable should be available"))
        .arg("--exact")
        .arg("process_chaos_child_crashes_during_requested_phase")
        .arg("--nocapture")
        .env(CHILD_MODE_ENV, crash_phase.as_env())
        .env(JOURNAL_ROOT_ENV, journal_root)
        .env(SHADOW_ROOT_ENV, shadow_root)
        .env(ADAPTER_ENDPOINT_ENV, endpoint)
        .output()
        .expect("child process should launch")
}

fn assert_child_crashed(output: Output, crash_phase: CrashPhase) {
    assert_eq!(
        output.status.code(),
        Some(CHILD_CRASH_EXIT_CODE),
        "child process should exit from {crash_phase:?} handler; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn start_phase_crash_adapter_at(
    addr: std::net::SocketAddr,
    crash_phase: CrashPhase,
) -> String {
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(UnderlayAdapterServer::new(PhaseCrashAdapter {
                crash_phase,
            }))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashPhase {
    Preparing,
    Committing,
    FinalConfirming,
}

impl CrashPhase {
    fn as_env(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Committing => "committing",
            Self::FinalConfirming => "final-confirming",
        }
    }

    fn from_env(value: &str) -> Option<Self> {
        match value {
            "preparing" => Some(Self::Preparing),
            "committing" => Some(Self::Committing),
            "final-confirming" => Some(Self::FinalConfirming),
            _ => None,
        }
    }
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

fn multi_endpoint_domain_intent(vlan_id: u16) -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "domain-process-chaos-multi".into(),
        topology: UnderlayTopology::SmallFabric,
        endpoints: vec![
            ManagementEndpointIntent {
                endpoint_id: "leaf-a".into(),
                host: "127.0.0.1".into(),
                port: 830,
                secret_ref: "local/leaf-a".into(),
                vendor_hint: Some(Vendor::Unknown),
                model_hint: None,
            },
            ManagementEndpointIntent {
                endpoint_id: "leaf-b".into(),
                host: "127.0.0.1".into(),
                port: 830,
                secret_ref: "local/leaf-b".into(),
                vendor_hint: Some(Vendor::Unknown),
                model_hint: None,
            },
        ],
        members: vec![
            SwitchMemberIntent {
                member_id: "member-a".into(),
                role: Some(DeviceRole::LeafA),
                management_endpoint_id: "leaf-a".into(),
            },
            SwitchMemberIntent {
                member_id: "member-b".into(),
                role: Some(DeviceRole::LeafB),
                management_endpoint_id: "leaf-b".into(),
            },
        ],
        vlans: vec![VlanIntent {
            vlan_id,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![
            InterfaceIntent {
                device_id: DeviceId("member-a".into()),
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Access { vlan_id },
            },
            InterfaceIntent {
                device_id: DeviceId("member-b".into()),
                name: "GE1/0/2".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Access { vlan_id },
            },
        ],
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

fn inventory_with_two_endpoints(endpoint_a: String, endpoint_b: String) -> DeviceInventory {
    let inventory = DeviceInventory::default();
    for (device_id, role, adapter_endpoint) in [
        ("leaf-a", DeviceRole::LeafA, endpoint_a),
        ("leaf-b", DeviceRole::LeafB, endpoint_b),
    ] {
        inventory
            .insert(DeviceInfo {
                tenant_id: "tenant-a".into(),
                site_id: "site-a".into(),
                id: DeviceId(device_id.into()),
                management_ip: "127.0.0.1".into(),
                management_port: 830,
                vendor_hint: Some(Vendor::Unknown),
                model_hint: None,
                role,
                secret_ref: format!("local/{device_id}"),
                host_key_policy: HostKeyPolicy::TrustOnFirstUse,
                adapter_endpoint,
                lifecycle_state: DeviceLifecycleState::Ready,
            })
            .expect("endpoint device should be inserted");
    }
    inventory
}

fn temp_store_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-process-chaos-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}

#[derive(Debug, Clone)]
struct PhaseCrashAdapter {
    crash_phase: CrashPhase,
}

#[async_trait]
impl UnderlayAdapter for PhaseCrashAdapter {
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
        if self.crash_phase == CrashPhase::Preparing {
            std::process::exit(CHILD_CRASH_EXIT_CODE);
        }
        Ok(Response::new(adapter::PrepareResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::Prepared)),
        }))
    }

    async fn commit(
        &self,
        _request: Request<adapter::CommitRequest>,
    ) -> Result<Response<adapter::CommitResponse>, Status> {
        if self.crash_phase == CrashPhase::Committing {
            std::process::exit(CHILD_CRASH_EXIT_CODE);
        }
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
        if self.crash_phase == CrashPhase::FinalConfirming {
            std::process::exit(CHILD_CRASH_EXIT_CODE);
        }
        Ok(Response::new(adapter::FinalConfirmResponse {
            result: Some(adapter_result(adapter::AdapterOperationStatus::Committed)),
        }))
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
