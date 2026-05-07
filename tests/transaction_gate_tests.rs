use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aria_underlay::api::request::{
    ApplyDomainIntentRequest, ApplyOptions, DriftAuditRequest, RefreshStateRequest,
};
use aria_underlay::api::response::ApplyStatus;
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::{AdminState, DeviceId, DeviceRole, PortMode, Vendor};
use aria_underlay::planner::domain_plan::plan_underlay_domain;
use aria_underlay::proto::adapter;
use aria_underlay::state::drift::DriftPolicy;
use aria_underlay::state::{
    DeviceShadowState, InMemoryShadowStateStore, JsonFileShadowStateStore, ShadowStateStore,
};
use aria_underlay::tx::{
    InMemoryTxJournalStore, JsonFileTxJournalStore, TxContext, TxJournalRecord, TxJournalStore,
    TxPhase,
};
use aria_underlay::{UnderlayError, UnderlayResult};

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
async fn apply_is_blocked_before_adapter_when_endpoint_has_pending_recoverable_transaction() {
    let inventory = inventory_with_endpoint("stack-mgmt", DeviceLifecycleState::Ready);
    let journal = Arc::new(InMemoryTxJournalStore::default());
    journal
        .put(
            &TxJournalRecord::started(
                &TxContext {
                    tx_id: "tx-prepared".into(),
                    request_id: "req-old".into(),
                    trace_id: "trace-old".into(),
                },
                vec![DeviceId("stack-mgmt".into())],
            )
            .with_phase(TxPhase::Prepared),
        )
        .expect("prepared journal record should be stored");
    let service = AriaUnderlayService::new_with_journal(inventory, journal);

    let response = service
        .apply_domain_intent(apply_request(DriftPolicy::ReportOnly))
        .await
        .expect("apply should return per-device blocking result");

    assert_eq!(response.status, ApplyStatus::InDoubt);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some("TX_REQUIRES_RECOVERY")
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

#[tokio::test]
async fn rollback_rpc_is_attempted_even_when_rolling_back_journal_write_fails() {
    let rollback_calls = Arc::new(AtomicUsize::new(0));
    let mut adapter = TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        commit_result: failed_result("COMMIT_FAILED"),
        rollback_calls: Some(rollback_calls.clone()),
        ..Default::default()
    };
    adapter.rollback_result = common::adapter_result(
        aria_underlay::proto::adapter::AdapterOperationStatus::RolledBack,
    );
    let endpoint = start_test_adapter(adapter).await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let journal = Arc::new(FailingRollingBackJournalStore::default());
    let service = AriaUnderlayService::new_with_journal(inventory, journal);

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("apply should return a per-device result even when journal write fails");

    assert_eq!(rollback_calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.status, ApplyStatus::Failed);
}

#[tokio::test]
async fn rollback_failure_preserves_prepare_failure_as_primary_error() {
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        prepare_result: failed_result("PREPARE_FAILED"),
        rollback_result: failed_result("ROLLBACK_FAILED"),
        ..Default::default()
    })
    .await;
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
        .expect("apply should return per-device rollback failure context");

    assert_eq!(response.status, ApplyStatus::InDoubt);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some("PREPARE_FAILED")
    );
    let message = response.device_results[0]
        .error_message
        .as_deref()
        .expect("error message should include rollback context");
    assert!(message.contains("rollback after endpoint failure also failed"));
    assert!(message.contains("ROLLBACK_FAILED"));

    let tx_id = response.device_results[0]
        .tx_id
        .as_deref()
        .expect("in-doubt transaction should include tx_id");
    let record = journal
        .get(tx_id)
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert_eq!(record.phase, TxPhase::InDoubt);
    assert_eq!(record.error_code.as_deref(), Some("PREPARE_FAILED"));
    assert!(
        record
            .error_history
            .iter()
            .any(|event| event.code == "ROLLBACK_FAILED"),
        "journal should retain rollback failure as secondary history"
    );
}

#[tokio::test]
async fn confirmed_commit_timeout_is_taken_from_service_configuration() {
    let commit_timeouts = Arc::new(Mutex::new(Vec::new()));
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        commit_confirm_timeouts: Some(commit_timeouts.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory).with_confirmed_commit_timeout_secs(45);

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("apply should succeed");

    assert_eq!(response.status, ApplyStatus::Success);
    assert_eq!(
        *commit_timeouts
            .lock()
            .expect("timeout recorder should not be poisoned"),
        vec![45]
    );
}

#[tokio::test]
async fn preflight_fetches_only_desired_scope_to_avoid_unrelated_delete_ops() {
    let current_state_scopes = Arc::new(Mutex::new(Vec::new()));
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_state_with_unrelated_objects()),
        current_state_scopes: Some(current_state_scopes.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory);
    let request = apply_request_with_vlan(200, DriftPolicy::ReportOnly);

    let dry_run = service
        .dry_run_domain(request.clone())
        .await
        .expect("dry-run should succeed");

    assert!(
        dry_run.change_sets[0]
            .ops
            .iter()
            .all(|op| !matches!(
                op,
                aria_underlay::engine::diff::ChangeOp::DeleteVlan { .. }
                    | aria_underlay::engine::diff::ChangeOp::DeleteInterfaceConfig { .. }
            )),
        "merge-upsert preflight should not plan deletes for unrelated observed state: {:?}",
        dry_run.change_sets
    );
    let response = service
        .apply_domain_intent(request)
        .await
        .expect("apply should succeed");
    assert_eq!(response.status, ApplyStatus::Success);

    let scopes = current_state_scopes
        .lock()
        .expect("current state scope recorder should not be poisoned");
    let scope_summaries = scopes
        .iter()
        .map(|scope| {
            format!(
                "full={} vlans={:?} interfaces={:?}",
                scope.full, scope.vlan_ids, scope.interface_names
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        scopes.iter().any(|scope| {
            !scope.full
                && scope.vlan_ids == vec![200]
                && scope.interface_names == vec!["GE1/0/1".to_string()]
        }),
        "preflight should request only desired scope, got {scope_summaries}"
    );
}

#[tokio::test]
async fn successful_device_apply_marks_transaction_in_doubt_when_shadow_update_fails() {
    let endpoint = start_fake_adapter(AdapterFailurePoint::None).await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let journal = Arc::new(InMemoryTxJournalStore::default());
    let service = AriaUnderlayService::new_with_shadow_store(
        inventory,
        journal.clone(),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        Arc::new(FailingDesiredShadowStore),
    );

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("shadow failure after adapter success should be returned as per-device result");

    assert_eq!(response.status, ApplyStatus::InDoubt);
    assert_eq!(response.device_results[0].status, ApplyStatus::InDoubt);
    assert_eq!(
        response.device_results[0].error_code.as_deref(),
        Some("INTERNAL")
    );
    let tx_id = response.device_results[0]
        .tx_id
        .as_deref()
        .expect("changed transaction should include tx_id");
    let record = journal
        .get(tx_id)
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert_eq!(record.phase, TxPhase::InDoubt);
    assert_eq!(record.error_code.as_deref(), Some("INTERNAL"));
}

#[tokio::test]
async fn successful_device_apply_persists_shadow_across_service_recreation() {
    let endpoint = start_fake_adapter(AdapterFailurePoint::None).await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let journal_root = temp_store_dir("journal");
    let shadow_root = temp_store_dir("shadow");
    let service = AriaUnderlayService::new_with_shadow_store(
        inventory,
        Arc::new(JsonFileTxJournalStore::new(&journal_root)),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        Arc::new(JsonFileShadowStateStore::new(&shadow_root)),
    );

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("successful fake adapter apply should complete");

    assert_eq!(response.status, ApplyStatus::Success);

    let restarted_shadow = JsonFileShadowStateStore::new(&shadow_root);
    let state = restarted_shadow
        .get(&DeviceId("stack-mgmt".into()))
        .expect("file shadow get should succeed after service recreation")
        .expect("file shadow should persist successful apply");

    assert_eq!(state.revision, 1);
    assert!(state.vlans.contains_key(&200));
    assert_eq!(
        state.interfaces["GE1/0/1"].mode,
        PortMode::Access { vlan_id: 200 }
    );

    std::fs::remove_dir_all(journal_root).ok();
    std::fs::remove_dir_all(shadow_root).ok();
}

#[tokio::test]
async fn refresh_state_does_not_replace_desired_shadow_baseline_for_drift_audit() {
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 200)),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let shadow_store = Arc::new(InMemoryShadowStateStore::default());
    shadow_store
        .put(desired_shadow_state(100))
        .expect("desired baseline should be stored");
    let service = AriaUnderlayService::new_with_shadow_store(
        inventory,
        Arc::new(InMemoryTxJournalStore::default()),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        shadow_store.clone(),
    );

    service
        .refresh_state(RefreshStateRequest {
            device_ids: vec![DeviceId("stack-mgmt".into())],
        })
        .await
        .expect("refresh should cache observed state separately");
    let response = service
        .run_drift_audit(DriftAuditRequest {
            device_ids: vec![DeviceId("stack-mgmt".into())],
        })
        .await
        .expect("drift audit should complete");

    assert_eq!(response.drifted_devices, vec![DeviceId("stack-mgmt".into())]);
    let baseline = shadow_store
        .get(&DeviceId("stack-mgmt".into()))
        .expect("shadow read should succeed")
        .expect("desired baseline should remain");
    assert!(baseline.vlans.contains_key(&100));
    assert!(!baseline.vlans.contains_key(&200));
}

#[tokio::test]
async fn clean_drift_audit_clears_previous_drift_lifecycle_state() {
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Drifted,
        endpoint,
    );
    let shadow_store = Arc::new(InMemoryShadowStateStore::default());
    shadow_store
        .put(desired_shadow_state(100))
        .expect("desired baseline should be stored");
    let service = AriaUnderlayService::new_with_shadow_store(
        inventory.clone(),
        Arc::new(InMemoryTxJournalStore::default()),
        Default::default(),
        Default::default(),
        Arc::new(aria_underlay::device::InMemorySecretStore::default()),
        shadow_store,
    );

    let response = service
        .run_drift_audit(DriftAuditRequest {
            device_ids: vec![DeviceId("stack-mgmt".into())],
        })
        .await
        .expect("clean drift audit should complete");

    assert!(response.drifted_devices.is_empty());
    let managed = inventory
        .get(&DeviceId("stack-mgmt".into()))
        .expect("inventory should still contain device");
    assert_eq!(managed.info.lifecycle_state, DeviceLifecycleState::Ready);
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
            ..Default::default()
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

fn desired_shadow_state(vlan_id: u16) -> DeviceShadowState {
    let desired = plan_underlay_domain(&domain_intent(vlan_id))
        .expect("domain intent should plan")
        .into_iter()
        .next()
        .expect("domain intent should produce one device");
    DeviceShadowState::from_desired(&desired, 0)
}

fn observed_state_with_unrelated_objects() -> adapter::ObservedDeviceState {
    adapter::ObservedDeviceState {
        device_id: "stack-mgmt".into(),
        vlans: vec![adapter::VlanConfig {
            vlan_id: 999,
            name: Some("unrelated".into()),
            description: None,
        }],
        interfaces: vec![adapter::InterfaceConfig {
            name: "GE1/0/2".into(),
            admin_state: adapter::AdminState::Up as i32,
            description: None,
            mode: Some(adapter::PortMode {
                kind: adapter::PortModeKind::Access as i32,
                access_vlan: Some(999),
                native_vlan: None,
                allowed_vlans: Vec::new(),
            }),
        }],
    }
}

fn temp_store_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria-underlay-transaction-{name}-{}", uuid::Uuid::new_v4()))
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
        AdapterFailurePoint::None => {}
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
    None,
    Prepare,
    Commit,
    Verify,
}

#[derive(Debug)]
struct FailingDesiredShadowStore;

impl ShadowStateStore for FailingDesiredShadowStore {
    fn get(&self, _device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        Ok(None)
    }

    fn put(&self, _state: DeviceShadowState) -> UnderlayResult<DeviceShadowState> {
        Err(UnderlayError::Internal("shadow store unavailable".into()))
    }

    fn remove(&self, _device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        Ok(None)
    }

    fn list(&self) -> UnderlayResult<Vec<DeviceShadowState>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
struct FailingRollingBackJournalStore {
    inner: InMemoryTxJournalStore,
}

impl TxJournalStore for FailingRollingBackJournalStore {
    fn put(&self, record: &TxJournalRecord) -> UnderlayResult<()> {
        if record.phase == TxPhase::RollingBack {
            return Err(UnderlayError::Internal("journal unavailable during rollback".into()));
        }
        self.inner.put(record)
    }

    fn get(&self, tx_id: &str) -> UnderlayResult<Option<TxJournalRecord>> {
        self.inner.get(tx_id)
    }

    fn list_recoverable(&self) -> UnderlayResult<Vec<TxJournalRecord>> {
        self.inner.list_recoverable()
    }
}
