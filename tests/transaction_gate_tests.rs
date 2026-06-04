use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aria_underlay::api::request::{
    ApplyDomainIntentRequest, ApplyOptions, DriftAuditRequest, RefreshStateRequest,
    RetryFailedDomainEndpointsRequest,
};
use aria_underlay::api::response::{ApplyStatus, ApplyVerifyStatus, DeviceVerifyStatus};
use aria_underlay::api::{AriaUnderlayService, UnderlayService};
use aria_underlay::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use aria_underlay::engine::change_plan::BlastRadius;
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

use common::{
    adapter_result, failed_result, observed_access_state, start_test_adapter, TestAdapter,
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
async fn successful_apply_returns_scoped_verify_report() {
    let endpoint = start_fake_adapter(AdapterFailurePoint::None).await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory);

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("apply should succeed");

    let device_report = response.device_results[0]
        .verify_report
        .as_ref()
        .expect("changed endpoint should include verify report");
    assert_eq!(device_report.status, DeviceVerifyStatus::Passed);
    assert_eq!(device_report.source, "adapter_scoped_verify");
    assert_eq!(device_report.scope.vlan_count, 1);
    assert_eq!(device_report.scope.interface_count, 1);
    assert!(device_report.error_code.is_none());

    let apply_report = response
        .verify_report
        .as_ref()
        .expect("apply response should include aggregate verify report");
    assert_eq!(apply_report.status, ApplyVerifyStatus::Passed);
    assert_eq!(apply_report.passed, vec![DeviceId("stack-mgmt".into())]);
    assert!(!apply_report.attention_required);
}

#[tokio::test]
async fn verify_failure_returns_failed_verify_report() {
    let endpoint = start_fake_adapter(AdapterFailurePoint::Verify).await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory);

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("verify failure should return a per-device result");

    assert_eq!(response.status, ApplyStatus::RolledBack);
    let device_report = response.device_results[0]
        .verify_report
        .as_ref()
        .expect("verify failure should include verify report");
    assert_eq!(device_report.status, DeviceVerifyStatus::Failed);
    assert_eq!(device_report.error_code.as_deref(), Some("VERIFY_FAILED"));
    assert_eq!(device_report.scope.vlan_count, 1);
    assert_eq!(device_report.scope.interface_count, 1);

    let apply_report = response
        .verify_report
        .as_ref()
        .expect("apply response should include aggregate verify report");
    assert_eq!(apply_report.status, ApplyVerifyStatus::Failed);
    assert_eq!(apply_report.failed, vec![DeviceId("stack-mgmt".into())]);
    assert!(apply_report.attention_required);
}

#[tokio::test]
async fn rollback_rpc_is_not_attempted_when_rolling_back_journal_write_fails() {
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

    assert_eq!(rollback_calls.load(Ordering::SeqCst), 0);
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
async fn prepared_candidate_checksum_is_sent_to_commit() {
    let commit_checksums = Arc::new(Mutex::new(Vec::new()));
    let mut prepare_result = adapter_result(adapter::AdapterOperationStatus::Prepared);
    prepare_result.prepared_candidate_checksum = "sha256:prepared".into();
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        prepare_result,
        commit_prepared_candidate_checksums: Some(commit_checksums.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory);

    let response = service
        .apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly))
        .await
        .expect("apply should succeed");

    assert_eq!(response.status, ApplyStatus::Success);
    assert_eq!(
        *commit_checksums
            .lock()
            .expect("checksum recorder should not be poisoned"),
        vec!["sha256:prepared".to_string()]
    );
}

#[tokio::test]
async fn apply_domain_intent_reuses_completed_response_for_same_idempotency_key() {
    let prepare_calls = Arc::new(AtomicUsize::new(0));
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        prepare_calls: Some(prepare_calls.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory);

    let mut first_request = apply_request_with_vlan(200, DriftPolicy::ReportOnly);
    first_request.idempotency_key = Some("tenant-a:site-a:op-1".into());
    let first_response = service
        .apply_domain_intent(first_request)
        .await
        .expect("first apply should succeed");

    let mut retry_request = apply_request_with_vlan(200, DriftPolicy::ReportOnly);
    retry_request.request_id = "req-apply-retry".into();
    retry_request.trace_id = Some("trace-apply-retry".into());
    retry_request.idempotency_key = Some("tenant-a:site-a:op-1".into());
    let retry_response = service
        .apply_domain_intent(retry_request)
        .await
        .expect("retry with same idempotency key should reuse the response");

    assert_eq!(prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_response.status, ApplyStatus::Success);
    assert_eq!(retry_response.status, ApplyStatus::Success);
    assert_eq!(first_response.tx_id, retry_response.tx_id);
    assert_eq!(
        retry_response.idempotency_key.as_deref(),
        Some("tenant-a:site-a:op-1")
    );
    assert!(!first_response.reused);
    assert!(retry_response.reused);
    assert_eq!(retry_response.request_id, "req-apply-retry");
    assert_eq!(retry_response.trace_id, "trace-apply-retry");
}

#[tokio::test]
async fn apply_domain_intent_rejects_same_idempotency_key_for_different_payload() {
    let prepare_calls = Arc::new(AtomicUsize::new(0));
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        prepare_calls: Some(prepare_calls.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let service = AriaUnderlayService::new(inventory);

    let mut first_request = apply_request_with_vlan(200, DriftPolicy::ReportOnly);
    first_request.idempotency_key = Some("tenant-a:site-a:op-2".into());
    service
        .apply_domain_intent(first_request)
        .await
        .expect("first apply should succeed");

    let mut different_request = apply_request_with_vlan(201, DriftPolicy::ReportOnly);
    different_request.request_id = "req-apply-different".into();
    different_request.idempotency_key = Some("tenant-a:site-a:op-2".into());
    let err = service
        .apply_domain_intent(different_request)
        .await
        .expect_err("same key with a different payload must fail closed");

    assert_eq!(prepare_calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        err,
        UnderlayError::InvalidIntent(message)
            if message.contains("idempotency_key")
                && message.contains("different apply payload")
    ));
}

#[tokio::test]
async fn apply_domain_intent_reuses_persisted_response_after_service_recreation() {
    let prepare_calls = Arc::new(AtomicUsize::new(0));
    let endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-mgmt", 100)),
        prepare_calls: Some(prepare_calls.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_at(
        "stack-mgmt",
        DeviceLifecycleState::Ready,
        endpoint,
    );
    let idempotency_root = temp_store_dir("idempotency");

    let first_service = AriaUnderlayService::new(inventory.clone())
        .with_file_apply_idempotency_store(&idempotency_root);
    let mut first_request = apply_request_with_vlan(200, DriftPolicy::ReportOnly);
    first_request.idempotency_key = Some("tenant-a:site-a:op-persisted".into());
    let first_response = first_service
        .apply_domain_intent(first_request)
        .await
        .expect("first apply should succeed and persist idempotency record");

    let restarted_service = AriaUnderlayService::new(inventory)
        .with_file_apply_idempotency_store(&idempotency_root);
    let mut retry_request = apply_request_with_vlan(200, DriftPolicy::ReportOnly);
    retry_request.request_id = "req-apply-after-restart".into();
    retry_request.trace_id = Some("trace-apply-after-restart".into());
    retry_request.idempotency_key = Some("tenant-a:site-a:op-persisted".into());
    let retry_response = restarted_service
        .apply_domain_intent(retry_request)
        .await
        .expect("retry after service recreation should reuse persisted response");

    assert_eq!(prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_response.tx_id, retry_response.tx_id);
    assert!(retry_response.reused);
    assert_eq!(retry_response.request_id, "req-apply-after-restart");
    assert_eq!(retry_response.trace_id, "trace-apply-after-restart");

    std::fs::remove_dir_all(idempotency_root).ok();
}

#[tokio::test]
async fn apply_domain_intent_serializes_same_domain_requests() {
    let first_prepare_calls = Arc::new(AtomicUsize::new(0));
    let first_prepare_release = Arc::new(tokio::sync::Notify::new());
    let first_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-a", 100)),
        prepare_calls: Some(first_prepare_calls.clone()),
        prepare_release: Some(first_prepare_release.clone()),
        ..Default::default()
    })
    .await;
    let second_prepare_calls = Arc::new(AtomicUsize::new(0));
    let second_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-b", 100)),
        prepare_calls: Some(second_prepare_calls.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_routes(&[
        ("stack-a", first_endpoint),
        ("stack-b", second_endpoint),
    ]);
    let service = AriaUnderlayService::new(inventory);

    let first_service = service.clone();
    let first = tokio::spawn(async move {
        first_service
            .apply_domain_intent(apply_request_for_domain_endpoint(
                "domain-a",
                "stack-a",
                200,
                DriftPolicy::ReportOnly,
            ))
            .await
    });
    wait_for_prepare_count(&first_prepare_calls, 1, "first apply should reach prepare").await;

    let second_service = service.clone();
    let second = tokio::spawn(async move {
        second_service
            .apply_domain_intent(apply_request_for_domain_endpoint(
                "domain-a",
                "stack-b",
                201,
                DriftPolicy::ReportOnly,
            ))
            .await
    });

    let second_reached_prepare = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        wait_until_prepare_count(&second_prepare_calls, 1),
    )
    .await
    .is_ok();
    assert!(
        !second_reached_prepare,
        "same-domain apply should wait before reaching the second endpoint prepare"
    );

    first_prepare_release.notify_one();
    let first_response = tokio::time::timeout(std::time::Duration::from_secs(3), first)
        .await
        .expect("first apply task should finish")
        .expect("first apply task should not panic")
        .expect("first apply should succeed");
    assert_eq!(first_response.status, ApplyStatus::Success);

    wait_for_prepare_count(
        &second_prepare_calls,
        1,
        "second apply should reach prepare after first releases the domain lock",
    )
    .await;
    let second_response = tokio::time::timeout(std::time::Duration::from_secs(3), second)
        .await
        .expect("second apply task should finish")
        .expect("second apply task should not panic")
        .expect("second apply should succeed");
    assert_eq!(second_response.status, ApplyStatus::Success);
}

#[tokio::test]
async fn apply_domain_intent_allows_different_domains_to_progress_independently() {
    let first_prepare_calls = Arc::new(AtomicUsize::new(0));
    let first_prepare_release = Arc::new(tokio::sync::Notify::new());
    let first_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-a", 100)),
        prepare_calls: Some(first_prepare_calls.clone()),
        prepare_release: Some(first_prepare_release.clone()),
        ..Default::default()
    })
    .await;
    let second_prepare_calls = Arc::new(AtomicUsize::new(0));
    let second_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-b", 100)),
        prepare_calls: Some(second_prepare_calls.clone()),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_routes(&[
        ("stack-a", first_endpoint),
        ("stack-b", second_endpoint),
    ]);
    let service = AriaUnderlayService::new(inventory);

    let first_service = service.clone();
    let first = tokio::spawn(async move {
        first_service
            .apply_domain_intent(apply_request_for_domain_endpoint(
                "domain-a",
                "stack-a",
                200,
                DriftPolicy::ReportOnly,
            ))
            .await
    });
    wait_for_prepare_count(&first_prepare_calls, 1, "first apply should reach prepare").await;

    let second_service = service.clone();
    let second = tokio::spawn(async move {
        second_service
            .apply_domain_intent(apply_request_for_domain_endpoint(
                "domain-b",
                "stack-b",
                201,
                DriftPolicy::ReportOnly,
            ))
            .await
    });

    wait_for_prepare_count(
        &second_prepare_calls,
        1,
        "different-domain apply should reach prepare while first domain is still active",
    )
    .await;
    let second_response = tokio::time::timeout(std::time::Duration::from_secs(3), second)
        .await
        .expect("second apply task should finish")
        .expect("second apply task should not panic")
        .expect("second apply should succeed");
    assert_eq!(second_response.status, ApplyStatus::Success);

    first_prepare_release.notify_one();
    let first_response = tokio::time::timeout(std::time::Duration::from_secs(3), first)
        .await
        .expect("first apply task should finish")
        .expect("first apply task should not panic")
        .expect("first apply should succeed");
    assert_eq!(first_response.status, ApplyStatus::Success);
}

#[tokio::test]
async fn retry_failed_domain_endpoints_replays_only_failed_endpoint() {
    let stack_a_prepare_calls = Arc::new(AtomicUsize::new(0));
    let stack_a_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-a", 100)),
        prepare_calls: Some(stack_a_prepare_calls.clone()),
        ..Default::default()
    })
    .await;
    let stack_b_prepare_calls = Arc::new(AtomicUsize::new(0));
    let stack_b_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-b", 100)),
        prepare_calls: Some(stack_b_prepare_calls.clone()),
        prepare_result: failed_result("PREPARE_FAILED"),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_routes(&[
        ("stack-a", stack_a_endpoint),
        ("stack-b", stack_b_endpoint),
    ]);
    let service = AriaUnderlayService::new(inventory);

    let response = service
        .apply_domain_intent(two_endpoint_apply_request("req-original", "trace-original"))
        .await
        .expect("partial apply should return per-endpoint results");

    assert_eq!(response.status, ApplyStatus::PartialSuccess);
    assert_eq!(stack_a_prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(stack_b_prepare_calls.load(Ordering::SeqCst), 1);

    let plan = service
        .get_domain_apply_compensation_plan("req-original")
        .expect("compensation plan should exist");
    assert_eq!(plan.completed, vec![DeviceId("stack-a".into())]);
    assert_eq!(plan.retryable_failed, vec![DeviceId("stack-b".into())]);
    assert!(plan.requires_recovery.is_empty());

    let retry_response = service
        .retry_failed_domain_endpoints(RetryFailedDomainEndpointsRequest {
            request_id: "req-retry".into(),
            trace_id: Some("trace-retry".into()),
            original_request_id: "req-original".into(),
            endpoint_ids: Vec::new(),
            idempotency_key: None,
        })
        .await
        .expect("retry should target only the failed endpoint");

    assert_eq!(retry_response.request_id, "req-retry");
    assert_eq!(retry_response.device_results.len(), 1);
    assert_eq!(retry_response.device_results[0].device_id, DeviceId("stack-b".into()));
    assert_eq!(stack_a_prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(stack_b_prepare_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn file_domain_apply_record_store_survives_service_recreation() {
    let stack_a_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-a", 100)),
        ..Default::default()
    })
    .await;
    let stack_b_endpoint = start_test_adapter(TestAdapter {
        current_state: Some(observed_access_state("stack-b", 100)),
        prepare_result: failed_result("PREPARE_FAILED"),
        ..Default::default()
    })
    .await;
    let inventory = inventory_with_endpoint_routes(&[
        ("stack-a", stack_a_endpoint),
        ("stack-b", stack_b_endpoint),
    ]);
    let apply_record_root = temp_store_dir("domain-apply-record");
    let service = AriaUnderlayService::new(inventory.clone())
        .with_file_domain_apply_record_store(&apply_record_root);

    let response = service
        .apply_domain_intent(two_endpoint_apply_request("req-persisted", "trace-persisted"))
        .await
        .expect("partial apply should persist an apply record");
    assert_eq!(response.status, ApplyStatus::PartialSuccess);

    let restarted = AriaUnderlayService::new(inventory)
        .with_file_domain_apply_record_store(&apply_record_root);
    let plan = restarted
        .get_domain_apply_compensation_plan("req-persisted")
        .expect("persisted compensation plan should load after service recreation");

    assert_eq!(plan.retryable_failed, vec![DeviceId("stack-b".into())]);
    std::fs::remove_dir_all(apply_record_root).ok();
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
            )),
        "merge-upsert preflight should not plan deletes for unrelated observed state: {:?}",
        dry_run.change_sets
    );
    assert_eq!(dry_run.change_plans.len(), dry_run.change_sets.len());
    assert_eq!(dry_run.change_plans[0].device_id, "stack-mgmt");
    assert_eq!(dry_run.change_plans[0].blast_radius, BlastRadius::LocalInterfaceOrVlan);
    assert!(!dry_run.change_plans[0].stages.is_empty());
    assert!(!dry_run.change_plans[0].rollback_order.is_empty());
    let json = serde_json::to_value(&dry_run).expect("dry-run response should serialize");
    assert!(json["change_plans"].is_array());
    assert_eq!(
        json["change_plans"][0]["blast_radius"],
        "local_interface_or_vlan"
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
    apply_request_for_domain_endpoint("domain-a", "stack-mgmt", vlan_id, drift_policy)
}

fn apply_request_for_domain_endpoint(
    domain_id: &str,
    endpoint_id: &str,
    vlan_id: u16,
    drift_policy: DriftPolicy,
) -> ApplyDomainIntentRequest {
    ApplyDomainIntentRequest {
        request_id: "req-apply".into(),
        trace_id: Some("trace-apply".into()),
        idempotency_key: None,
        intent: domain_intent_for_endpoint(domain_id, endpoint_id, vlan_id),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
            drift_policy,
            ..Default::default()
        },
    }
}

fn domain_intent(vlan_id: u16) -> UnderlayDomainIntent {
    domain_intent_for_endpoint("domain-a", "stack-mgmt", vlan_id)
}

fn domain_intent_for_endpoint(
    domain_id: &str,
    endpoint_id: &str,
    vlan_id: u16,
) -> UnderlayDomainIntent {
    let member_id = format!("{endpoint_id}-member");
    UnderlayDomainIntent {
        domain_id: domain_id.into(),
        topology: UnderlayTopology::StackSingleManagementIp,
        endpoints: vec![ManagementEndpointIntent {
            endpoint_id: endpoint_id.into(),
            host: "127.0.0.1".into(),
            port: 830,
            secret_ref: format!("local/{endpoint_id}"),
            vendor_hint: Some(Vendor::Unknown),
            model_hint: None,
        }],
        members: vec![SwitchMemberIntent {
            member_id: member_id.clone(),
            role: Some(DeviceRole::LeafA),
            management_endpoint_id: endpoint_id.into(),
        }],
        vlans: vec![VlanIntent {
            vlan_id,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![InterfaceIntent {
            device_id: DeviceId(member_id),
            name: "GE1/0/1".into(),
            admin_state: AdminState::Up,
            description: None,
            mode: PortMode::Access { vlan_id },
        }],
        acls: vec![],
        acl_bindings: vec![],
        delete_vlan_ids: vec![],
        delete_interfaces: vec![],
        delete_acl_ids: vec![],
        delete_acl_bindings: vec![],
    }
}

fn two_endpoint_apply_request(request_id: &str, trace_id: &str) -> ApplyDomainIntentRequest {
    ApplyDomainIntentRequest {
        request_id: request_id.into(),
        trace_id: Some(trace_id.into()),
        idempotency_key: None,
        intent: two_endpoint_domain_intent(),
        options: ApplyOptions {
            dry_run: false,
            allow_degraded_atomicity: false,
            drift_policy: DriftPolicy::ReportOnly,
            ..Default::default()
        },
    }
}

fn two_endpoint_domain_intent() -> UnderlayDomainIntent {
    UnderlayDomainIntent {
        domain_id: "domain-a".into(),
        topology: UnderlayTopology::MlagDualManagementIp,
        endpoints: vec![
            ManagementEndpointIntent {
                endpoint_id: "stack-a".into(),
                host: "127.0.0.1".into(),
                port: 830,
                secret_ref: "local/stack-a".into(),
                vendor_hint: Some(Vendor::Unknown),
                model_hint: None,
            },
            ManagementEndpointIntent {
                endpoint_id: "stack-b".into(),
                host: "127.0.0.1".into(),
                port: 830,
                secret_ref: "local/stack-b".into(),
                vendor_hint: Some(Vendor::Unknown),
                model_hint: None,
            },
        ],
        members: vec![
            SwitchMemberIntent {
                member_id: "member-a".into(),
                role: Some(DeviceRole::LeafA),
                management_endpoint_id: "stack-a".into(),
            },
            SwitchMemberIntent {
                member_id: "member-b".into(),
                role: Some(DeviceRole::LeafB),
                management_endpoint_id: "stack-b".into(),
            },
        ],
        vlans: vec![VlanIntent {
            vlan_id: 200,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![
            InterfaceIntent {
                device_id: DeviceId("member-a".into()),
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Access { vlan_id: 200 },
            },
            InterfaceIntent {
                device_id: DeviceId("member-b".into()),
                name: "GE1/0/1".into(),
                admin_state: AdminState::Up,
                description: None,
                mode: PortMode::Access { vlan_id: 200 },
            },
        ],
        acls: Vec::new(),
        acl_bindings: Vec::new(),
        delete_vlan_ids: Vec::new(),
        delete_interfaces: Vec::new(),
        delete_acl_ids: Vec::new(),
        delete_acl_bindings: Vec::new(),
    }
}

async fn wait_for_prepare_count(calls: &AtomicUsize, expected: usize, context: &str) {
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        wait_until_prepare_count(calls, expected),
    )
    .await
    .expect(context);
}

async fn wait_until_prepare_count(calls: &AtomicUsize, expected: usize) {
    loop {
        if calls.load(Ordering::SeqCst) >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
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
        acls: Vec::new(),
        acl_bindings: Vec::new(),
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
    insert_inventory_endpoint(&inventory, device_id, state, adapter_endpoint);
    inventory
}

fn inventory_with_endpoint_routes(routes: &[(&str, String)]) -> DeviceInventory {
    let inventory = DeviceInventory::default();
    for (device_id, adapter_endpoint) in routes {
        insert_inventory_endpoint(
            &inventory,
            device_id,
            DeviceLifecycleState::Ready,
            adapter_endpoint.clone(),
        );
    }
    inventory
}

fn insert_inventory_endpoint(
    inventory: &DeviceInventory,
    device_id: &str,
    state: DeviceLifecycleState,
    adapter_endpoint: String,
) {
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
