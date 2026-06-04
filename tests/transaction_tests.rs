use aria_underlay::api::apply_compensation::{
    filter_domain_intent_to_endpoints, DomainApplyCompensationPlan, DomainApplyRecord,
    JsonFileDomainApplyRecordStore,
};
use aria_underlay::api::request::{ApplyDomainIntentRequest, ApplyOptions};
use aria_underlay::api::response::{
    ApplyIntentResponse, ApplyStatus, ApplyVerifyReport, ApplyVerifyStatus, DeviceApplyResult,
    DeviceVerifyReport, DeviceVerifyStatus, VerifyScopeSummary,
};
use aria_underlay::intent::interface::InterfaceIntent;
use aria_underlay::intent::vlan::VlanIntent;
use aria_underlay::intent::{
    ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent, UnderlayTopology,
};
use aria_underlay::model::DeviceId;
use aria_underlay::model::{AdminState, DeviceRole, PortMode, Vendor};
use aria_underlay::tx::{
    choose_strategy, CapabilityFlags, DomainApplyLockTable, EndpointLockTable,
    JsonFileTxJournalStore, LockAcquisitionPolicy, TransactionMode, TransactionStrategy,
    TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[test]
fn confirmed_commit_strategy_wins_when_supported() {
    let strategy = choose_strategy(
        CapabilityFlags {
            supports_candidate: true,
            supports_validate: true,
            supports_confirmed_commit: true,
            supports_persist_id: true,
            supports_rollback_on_error: false,
            supports_writable_running: false,
            supports_cli_fallback: false,
        },
        TransactionMode::StrictConfirmedCommit,
    );

    assert_eq!(strategy, TransactionStrategy::ConfirmedCommit);
}

#[test]
fn file_journal_round_trips_record() {
    let root = temp_journal_dir("round-trip");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-1".into(),
        request_id: "req-1".into(),
        trace_id: "trace-1".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_strategy(TransactionStrategy::ConfirmedCommit)
        .with_phase(TxPhase::Prepared);

    store.put(&record).expect("journal put should succeed");
    let loaded = store
        .get("tx-1")
        .expect("journal get should succeed")
        .expect("record should exist");

    assert_eq!(loaded.tx_id, "tx-1");
    assert_eq!(loaded.request_id, "req-1");
    assert_eq!(loaded.trace_id, "trace-1");
    assert_eq!(loaded.phase, TxPhase::Prepared);
    assert_eq!(loaded.strategy, Some(TransactionStrategy::ConfirmedCommit));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn journal_record_transition_phase_updates_phase_and_timestamp() {
    let context = TxContext {
        tx_id: "tx-transition".into(),
        request_id: "req-transition".into(),
        trace_id: "trace-transition".into(),
    };
    let mut record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);
    let original_updated_at = record.updated_at_unix_secs;

    record
        .transition_phase(TxPhase::Preparing)
        .expect("Started -> Preparing should be valid");

    assert_eq!(record.phase, TxPhase::Preparing);
    assert!(record.updated_at_unix_secs >= original_updated_at);
}

#[test]
fn journal_record_transition_phase_rejects_invalid_skip() {
    let context = TxContext {
        tx_id: "tx-invalid-transition".into(),
        request_id: "req-invalid-transition".into(),
        trace_id: "trace-invalid-transition".into(),
    };
    let mut record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    let err = record
        .transition_phase(TxPhase::Committed)
        .expect_err("Started -> Committed should be invalid");

    assert_eq!(record.phase, TxPhase::Started);
    assert!(err.to_string().contains("Started -> Committed"));
}

#[test]
fn journal_record_transition_phase_preserves_committed_to_in_doubt_recovery_semantics() {
    let context = TxContext {
        tx_id: "tx-committed-shadow-failure".into(),
        request_id: "req-committed-shadow-failure".into(),
        trace_id: "trace-committed-shadow-failure".into(),
    };
    let mut record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    record.transition_phase(TxPhase::Preparing).unwrap();
    record.transition_phase(TxPhase::Prepared).unwrap();
    record.transition_phase(TxPhase::Committing).unwrap();
    record.transition_phase(TxPhase::Verifying).unwrap();
    record.transition_phase(TxPhase::FinalConfirming).unwrap();
    record.transition_phase(TxPhase::Committed).unwrap();
    record
        .transition_phase(TxPhase::InDoubt)
        .expect("Committed -> InDoubt is required for post-commit shadow failure");

    assert_eq!(record.phase, TxPhase::InDoubt);
}

#[test]
fn journal_record_preserves_error_history() {
    let context = TxContext {
        tx_id: "tx-errors".into(),
        request_id: "req-errors".into(),
        trace_id: "trace-errors".into(),
    };

    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::Committing)
        .with_error("COMMIT_FAILED", "commit failed")
        .with_phase(TxPhase::InDoubt)
        .with_error("ROLLBACK_FAILED", "rollback failed");

    assert_eq!(record.error_code.as_deref(), Some("ROLLBACK_FAILED"));
    assert_eq!(record.error_history.len(), 2);
    assert_eq!(record.error_history[0].phase, TxPhase::Committing);
    assert_eq!(record.error_history[0].code, "COMMIT_FAILED");
    assert_eq!(record.error_history[1].phase, TxPhase::InDoubt);
    assert_eq!(record.error_history[1].code, "ROLLBACK_FAILED");
}

#[test]
fn file_journal_round_trips_error_history() {
    let root = temp_journal_dir("error-history");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-error-history".into(),
        request_id: "req-error-history".into(),
        trace_id: "trace-error-history".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::Committing)
        .with_error("COMMIT_FAILED", "commit failed")
        .with_phase(TxPhase::InDoubt)
        .with_error("ROLLBACK_FAILED", "rollback failed");

    store.put(&record).expect("journal put should succeed");
    let loaded = store
        .get("tx-error-history")
        .expect("journal get should succeed")
        .expect("record should exist");

    assert_eq!(loaded.error_history.len(), 2);
    assert_eq!(loaded.error_history[0].code, "COMMIT_FAILED");
    assert_eq!(loaded.error_history[1].code, "ROLLBACK_FAILED");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_round_trips_manual_resolution() {
    let root = temp_journal_dir("manual-resolution");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-manual-resolution".into(),
        request_id: "req-manual-resolution".into(),
        trace_id: "trace-manual-resolution".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::InDoubt)
        .with_manual_resolution(
            "netops-a",
            "validated device state out of band",
            "req-force",
            "trace-force",
        )
        .with_phase(TxPhase::ForceResolved);

    store.put(&record).expect("journal put should succeed");
    let loaded = store
        .get("tx-manual-resolution")
        .expect("journal get should succeed")
        .expect("record should exist");

    assert_eq!(loaded.phase, TxPhase::ForceResolved);
    let manual = loaded
        .manual_resolution
        .expect("manual resolution should round-trip through file journal");
    assert_eq!(manual.operator, "netops-a");
    assert_eq!(manual.reason, "validated device state out of band");
    assert_eq!(manual.request_id, "req-force");
    assert_eq!(manual.trace_id, "trace-force");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_lists_only_recoverable_records() {
    let root = temp_journal_dir("recoverable");
    let store = JsonFileTxJournalStore::new(&root);
    let active = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-active".into(),
            request_id: "req-active".into(),
            trace_id: "trace-active".into(),
        },
        vec![DeviceId("leaf-a".into())],
    )
    .with_phase(TxPhase::Verifying);
    let committed = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-committed".into(),
            request_id: "req-committed".into(),
            trace_id: "trace-committed".into(),
        },
        vec![DeviceId("leaf-b".into())],
    )
    .with_phase(TxPhase::Committed);

    store.put(&active).expect("active journal put should succeed");
    store
        .put(&committed)
        .expect("committed journal put should succeed");

    let recoverable = store
        .list_recoverable()
        .expect("journal list should succeed");

    assert_eq!(recoverable.len(), 1);
    assert_eq!(recoverable[0].tx_id, "tx-active");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_terminal_records_stay_non_recoverable_after_store_recreation() {
    let root = temp_journal_dir("terminal-restart");
    let store = JsonFileTxJournalStore::new(&root);
    let terminal_records = [
        ("tx-committed", TxPhase::Committed),
        ("tx-failed", TxPhase::Failed),
        ("tx-rolled-back", TxPhase::RolledBack),
        ("tx-force-resolved", TxPhase::ForceResolved),
    ];

    for (tx_id, phase) in &terminal_records {
        let record = TxJournalRecord::started(
            &TxContext {
                tx_id: (*tx_id).into(),
                request_id: format!("req-{tx_id}"),
                trace_id: format!("trace-{tx_id}"),
            },
            vec![DeviceId("leaf-a".into())],
        )
        .with_phase(phase.clone());

        store
            .put(&record)
            .expect("terminal journal put should succeed");
    }

    let restarted = JsonFileTxJournalStore::new(&root);
    let recoverable = restarted
        .list_recoverable()
        .expect("journal restart scan should succeed");

    assert!(recoverable.is_empty());
    for (tx_id, phase) in &terminal_records {
        let loaded = restarted
            .get(tx_id)
            .expect("terminal journal get should succeed")
            .expect("terminal journal should survive restart");
        assert_eq!(&loaded.phase, phase);
    }

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_rejects_corrupt_record_during_restart_scan() {
    let root = temp_journal_dir("corrupt-restart");
    std::fs::create_dir_all(&root).expect("journal root should be created");
    std::fs::write(root.join("tx-corrupt.json"), b"{not valid json")
        .expect("corrupt journal fixture should be written");

    let restarted = JsonFileTxJournalStore::new(&root);
    let err = restarted
        .list_recoverable()
        .expect_err("corrupt journal record should fail closed during recovery scan");
    let message = format!("{err}");

    assert!(
        message.contains("parse tx journal"),
        "unexpected journal parse error: {message}"
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_ignores_tmp_crash_residue_after_store_recreation() {
    let root = temp_journal_dir("tmp-residue");
    let store = JsonFileTxJournalStore::new(&root);
    let active = TxJournalRecord::started(
        &TxContext {
            tx_id: "tx-active".into(),
            request_id: "req-active".into(),
            trace_id: "trace-active".into(),
        },
        vec![DeviceId("leaf-a".into())],
    )
    .with_phase(TxPhase::Preparing);

    store.put(&active).expect("active journal put should succeed");
    std::fs::write(root.join(".tx-active.json.leftover.tmp"), b"not json")
        .expect("tmp journal residue should be written");

    let restarted = JsonFileTxJournalStore::new(&root);
    let recoverable = restarted
        .list_recoverable()
        .expect("journal restart scan should ignore tmp residue");

    assert_eq!(recoverable.len(), 1);
    assert_eq!(recoverable[0].tx_id, "tx-active");

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_rejects_invalid_transaction_id_path() {
    let root = temp_journal_dir("sanitize");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "../bad/tx".into(),
        request_id: "req-1".into(),
        trace_id: "trace-1".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    let err = store
        .put(&record)
        .expect_err("invalid tx_id should be rejected instead of sanitized");

    assert!(
        format!("{err}").contains("invalid for file journal store"),
        "unexpected tx_id validation error: {err}"
    );
    assert!(!root.join("___bad_tx.json").exists());
    assert!(store
        .get("../bad/tx")
        .expect_err("invalid get should fail")
        .to_string()
        .contains("invalid"));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_serializes_concurrent_same_transaction_writes() {
    let root = temp_journal_dir("concurrent");
    let store = Arc::new(JsonFileTxJournalStore::new(&root));

    let writers = (0..24)
        .map(|index| {
            let store = store.clone();
            std::thread::spawn(move || {
                let context = TxContext {
                    tx_id: "tx-concurrent".into(),
                    request_id: format!("req-{index}"),
                    trace_id: format!("trace-{index}"),
                };
                let phase = if index % 2 == 0 {
                    TxPhase::Preparing
                } else {
                    TxPhase::Verifying
                };
                let record =
                    TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
                        .with_phase(phase);

                store
                    .put(&record)
                    .expect("concurrent file journal put should succeed");
            })
        })
        .collect::<Vec<_>>();

    for writer in writers {
        writer
            .join()
            .expect("journal writer thread should not panic");
    }

    let loaded = store
        .get("tx-concurrent")
        .expect("journal get should succeed")
        .expect("journal record should exist");
    assert!(loaded.request_id.starts_with("req-"));
    assert!(
        std::fs::read_dir(&root)
            .expect("journal root should be readable")
            .all(|entry| !entry
                .expect("journal entry should be readable")
                .path()
                .to_string_lossy()
                .ends_with(".tmp"))
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn file_journal_prunes_idle_lock_entries_after_write() {
    let root = temp_journal_dir("lock-prune");
    let store = JsonFileTxJournalStore::new(&root);
    let context = TxContext {
        tx_id: "tx-lock-prune".into(),
        request_id: "req-lock-prune".into(),
        trace_id: "trace-lock-prune".into(),
    };
    let record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())])
        .with_phase(TxPhase::Committed);

    store.put(&record).expect("journal put should succeed");

    assert_eq!(
        store.lock_entry_count(),
        0,
        "idle file-journal locks should not accumulate after writes"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn endpoint_lock_serializes_same_endpoint_writers() {
    let locks = EndpointLockTable::default();
    let first_guard = locks
        .acquire_many(&[DeviceId("leaf-a".into())])
        .await
        .expect("first lock should be acquired");
    let acquired = Arc::new(AtomicBool::new(false));
    let second_acquired = acquired.clone();
    let second_locks = locks.clone();

    let second = tokio::spawn(async move {
        let _guard = second_locks
            .acquire_many(&[DeviceId("leaf-a".into())])
            .await
            .expect("second lock should eventually be acquired");
        second_acquired.store(true, Ordering::SeqCst);
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!acquired.load(Ordering::SeqCst));

    drop(first_guard);
    second.await.expect("second lock task should finish");
    assert!(acquired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn endpoint_lock_orders_multiple_endpoints_without_deadlock() {
    let locks = EndpointLockTable::default();
    let first_locks = locks.clone();
    let second_locks = locks.clone();

    let first = tokio::spawn(async move {
        let _guard = first_locks
            .acquire_many(&[DeviceId("leaf-b".into()), DeviceId("leaf-a".into())])
            .await
            .expect("first multi endpoint lock should be acquired");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    });
    let second = tokio::spawn(async move {
        let _guard = second_locks
            .acquire_many(&[DeviceId("leaf-a".into()), DeviceId("leaf-b".into())])
            .await
            .expect("second multi endpoint lock should be acquired");
    });

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        first.await.expect("first lock task should finish");
        second.await.expect("second lock task should finish");
    })
    .await
    .expect("ordered endpoint locking should not deadlock");
}

#[tokio::test]
async fn endpoint_lock_policy_times_out_instead_of_waiting_forever() {
    let locks = EndpointLockTable::default();
    let _first_guard = locks
        .acquire_many(&[DeviceId("leaf-a".into())])
        .await
        .expect("first lock should be acquired");
    let policy = LockAcquisitionPolicy {
        max_wait_secs: 0,
        initial_delay_ms: 1,
        max_delay_secs: 1,
        jitter: false,
        force_unlock_enabled: false,
    };

    let err = locks
        .acquire_many_with_policy(&[DeviceId("leaf-a".into())], &policy)
        .await
        .expect_err("second lock should time out");

    assert!(format!("{err}").contains("ENDPOINT_LOCK_TIMEOUT"));
}

#[tokio::test]
async fn domain_apply_lock_serializes_same_domain_writers() {
    let locks = DomainApplyLockTable::default();
    let first_guard = locks
        .acquire("domain-a")
        .await
        .expect("first domain lock should be acquired");
    let acquired = Arc::new(AtomicBool::new(false));
    let second_acquired = acquired.clone();
    let second_locks = locks.clone();

    let second = tokio::spawn(async move {
        let _guard = second_locks
            .acquire("domain-a")
            .await
            .expect("second domain lock should eventually be acquired");
        second_acquired.store(true, Ordering::SeqCst);
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!acquired.load(Ordering::SeqCst));

    drop(first_guard);
    second.await.expect("second lock task should finish");
    assert!(acquired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn domain_apply_lock_allows_different_domains_to_run_concurrently() {
    let locks = DomainApplyLockTable::default();
    let _first_guard = locks
        .acquire("domain-a")
        .await
        .expect("first domain lock should be acquired");

    let _second_guard = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        locks.acquire("domain-b"),
    )
    .await
    .expect("different domain should not wait")
    .expect("second domain lock should be acquired");
}

#[test]
fn compensation_plan_classifies_terminal_failed_and_in_doubt_endpoints() {
    let response = ApplyIntentResponse {
        request_id: "req-original".into(),
        trace_id: "trace-original".into(),
        idempotency_key: None,
        reused: false,
        tx_id: None,
        status: ApplyStatus::PartialSuccess,
        strategy: None,
        device_results: vec![
            device_result("stack-a", ApplyStatus::Success),
            device_result("stack-b", ApplyStatus::RolledBack),
            device_result("stack-c", ApplyStatus::InDoubt),
        ],
        warnings: Vec::new(),
        verify_report: None,
    };

    let plan = DomainApplyCompensationPlan::from_record(&DomainApplyRecord::new(
        apply_record_request(two_endpoint_domain_intent()),
        response,
    ));

    assert_eq!(plan.completed, vec![DeviceId("stack-a".into())]);
    assert_eq!(plan.retryable_failed, vec![DeviceId("stack-b".into())]);
    assert_eq!(plan.requires_recovery, vec![DeviceId("stack-c".into())]);
}

#[test]
fn compensation_filter_keeps_only_selected_endpoint_scope() {
    let filtered = filter_domain_intent_to_endpoints(
        &two_endpoint_domain_intent(),
        &[DeviceId("stack-b".into())],
    )
    .expect("filtering to stack-b should succeed");

    assert_eq!(filtered.endpoints.len(), 1);
    assert_eq!(filtered.endpoints[0].endpoint_id, "stack-b");
    assert_eq!(filtered.topology, UnderlayTopology::StackSingleManagementIp);
    assert_eq!(filtered.members.len(), 1);
    assert_eq!(filtered.members[0].member_id, "member-b");
    assert_eq!(filtered.interfaces.len(), 1);
    assert_eq!(filtered.interfaces[0].device_id, DeviceId("member-b".into()));
    assert_eq!(filtered.vlans.len(), 1);
    assert_eq!(filtered.vlans[0].vlan_id, 200);
}

#[test]
fn aggregate_verify_report_marks_partial_when_some_endpoints_fail_verify() {
    let report = ApplyVerifyReport::from_device_results(&[
        device_result_with_verify("leaf-a", DeviceVerifyStatus::Passed),
        device_result_with_verify("leaf-b", DeviceVerifyStatus::Failed),
    ]);

    assert_eq!(report.status, ApplyVerifyStatus::Partial);
    assert_eq!(report.passed, vec![DeviceId("leaf-a".into())]);
    assert_eq!(report.failed, vec![DeviceId("leaf-b".into())]);
    assert!(report.attention_required);
}

#[test]
fn aggregate_verify_report_marks_in_doubt_when_any_endpoint_is_in_doubt() {
    let report = ApplyVerifyReport::from_device_results(&[
        device_result_with_verify("leaf-a", DeviceVerifyStatus::Passed),
        device_result_with_verify("leaf-b", DeviceVerifyStatus::InDoubt),
    ]);

    assert_eq!(report.status, ApplyVerifyStatus::InDoubt);
    assert_eq!(report.in_doubt, vec![DeviceId("leaf-b".into())]);
    assert!(report.attention_required);
}

#[test]
fn legacy_apply_response_without_verify_report_deserializes() {
    let json = r#"{
        "request_id": "req-legacy",
        "trace_id": "trace-legacy",
        "tx_id": null,
        "status": "Success",
        "strategy": null,
        "device_results": [{
            "device_id": "leaf-a",
            "changed": true,
            "status": "Success",
            "tx_id": "tx-legacy",
            "strategy": "ConfirmedCommit",
            "error_code": null,
            "error_message": null,
            "warnings": []
        }],
        "warnings": []
    }"#;

    let response: ApplyIntentResponse =
        serde_json::from_str(json).expect("legacy response should remain compatible");

    assert!(response.verify_report.is_none());
    assert!(response.device_results[0].verify_report.is_none());
}

#[test]
fn file_domain_apply_record_store_round_trips_records() {
    let root = temp_journal_dir("apply-record");
    let store = JsonFileDomainApplyRecordStore::new(&root);
    let request = apply_record_request(two_endpoint_domain_intent());
    let response = ApplyIntentResponse {
        request_id: "req-original".into(),
        trace_id: "trace-original".into(),
        idempotency_key: None,
        reused: false,
        tx_id: None,
        status: ApplyStatus::PartialSuccess,
        strategy: None,
        device_results: vec![
            device_result("stack-a", ApplyStatus::Success),
            device_result("stack-b", ApplyStatus::RolledBack),
        ],
        warnings: Vec::new(),
        verify_report: None,
    };
    let record = DomainApplyRecord::new(request, response);

    store.put(&record).expect("apply record should persist");
    let restarted = JsonFileDomainApplyRecordStore::new(&root);
    let loaded = restarted
        .get("req-original")
        .expect("apply record get should succeed")
        .expect("apply record should exist");

    assert_eq!(loaded.request.request_id, "req-original");
    assert_eq!(loaded.domain_id, "domain-a");
    assert_eq!(loaded.response.status, ApplyStatus::PartialSuccess);

    std::fs::remove_dir_all(root).ok();
}

fn device_result(device_id: &str, status: ApplyStatus) -> DeviceApplyResult {
    DeviceApplyResult {
        device_id: DeviceId(device_id.into()),
        changed: !matches!(&status, ApplyStatus::NoOpSuccess),
        status,
        tx_id: None,
        strategy: None,
        error_code: None,
        error_message: None,
        warnings: Vec::new(),
        verify_report: None,
    }
}

fn device_result_with_verify(device_id: &str, status: DeviceVerifyStatus) -> DeviceApplyResult {
    let mut result = device_result(device_id, ApplyStatus::Success);
    result.verify_report = Some(DeviceVerifyReport {
        device_id: DeviceId(device_id.into()),
        status,
        source: "adapter_scoped_verify".into(),
        scope: VerifyScopeSummary::default(),
        warnings: Vec::new(),
        error_code: None,
        error_message: None,
    });
    result
}

fn apply_record_request(intent: UnderlayDomainIntent) -> ApplyDomainIntentRequest {
    ApplyDomainIntentRequest {
        request_id: "req-original".into(),
        trace_id: Some("trace-original".into()),
        idempotency_key: None,
        intent,
        options: ApplyOptions::default(),
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

fn temp_journal_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria-underlay-journal-{name}-{}", uuid::Uuid::new_v4()))
}
