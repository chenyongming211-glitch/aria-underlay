use async_trait::async_trait;
use std::sync::Arc;

use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::adapter_client::{tx_request_context, AdapterClient};
use crate::api::force_resolve::{
    ForceResolveTransactionRequest, ForceResolveTransactionResponse,
};
use crate::api::force_unlock::{ForceUnlockRequest, ForceUnlockResponse};
use crate::api::request::{
    ApplyDomainIntentRequest, ApplyIntentRequest, DriftAuditRequest, RefreshStateRequest,
};
use crate::api::response::{
    ApplyIntentResponse, ApplyStatus, DeviceApplyResult, DeviceOnboardingResponse,
    DriftAuditResponse, DryRunResponse, RefreshStateResponse,
};
use crate::api::underlay_service::UnderlayService;
use crate::device::{
    DeviceInfo, DeviceInventory, DeviceLifecycleState, DeviceOnboardingService,
    DeviceRegistrationService, InMemorySecretStore, InitializeUnderlaySiteRequest,
    InitializeUnderlaySiteResponse, RegisterDeviceRequest, RegisterDeviceResponse, SecretStore,
    UnderlaySiteInitializationService,
};
use crate::engine::dry_run::{build_dry_run_plan, DryRunPlan};
use crate::intent::validation::validate_switch_pair_intent;
use crate::model::DeviceId;
use crate::planner::device_plan::{plan_switch_pair, DeviceDesiredState};
use crate::planner::domain_plan::plan_underlay_domain;
use crate::proto::adapter::RequestContext;
use crate::state::drift::{detect_drift, DriftPolicy, DriftReport};
use crate::state::{
    DeviceShadowState, InMemoryShadowStateStore, ShadowStateStore,
};
use crate::telemetry::{EventSink, NoopEventSink, UnderlayEvent};
use crate::tx::recovery::{
    classify_recovery, in_doubt_records_for_devices, RecoveryAction, RecoveryReport,
};
use crate::tx::{
    EndpointLockTable, InMemoryTxJournalStore, LockAcquisitionPolicy, TransactionStrategy,
    TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct AriaUnderlayService {
    inventory: DeviceInventory,
    journal: Arc<dyn TxJournalStore>,
    endpoint_locks: EndpointLockTable,
    lock_policy: LockAcquisitionPolicy,
    secret_store: Arc<dyn SecretStore>,
    shadow_store: Arc<dyn ShadowStateStore>,
    event_sink: Arc<dyn EventSink>,
}

impl AriaUnderlayService {
    pub fn new(inventory: DeviceInventory) -> Self {
        Self {
            inventory,
            journal: Arc::new(InMemoryTxJournalStore::default()),
            endpoint_locks: EndpointLockTable::default(),
            lock_policy: LockAcquisitionPolicy::default(),
            secret_store: Arc::new(InMemorySecretStore::default()),
            shadow_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
        }
    }

    pub fn new_with_journal(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks: EndpointLockTable::default(),
            lock_policy: LockAcquisitionPolicy::default(),
            secret_store: Arc::new(InMemorySecretStore::default()),
            shadow_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
        }
    }

    pub fn new_with_journal_and_locks(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
        endpoint_locks: EndpointLockTable,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks,
            lock_policy: LockAcquisitionPolicy::default(),
            secret_store: Arc::new(InMemorySecretStore::default()),
            shadow_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
        }
    }

    pub fn new_with_components(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
        endpoint_locks: EndpointLockTable,
        lock_policy: LockAcquisitionPolicy,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks,
            lock_policy,
            secret_store,
            shadow_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
        }
    }

    pub fn new_with_shadow_store(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
        endpoint_locks: EndpointLockTable,
        lock_policy: LockAcquisitionPolicy,
        secret_store: Arc<dyn SecretStore>,
        shadow_store: Arc<dyn ShadowStateStore>,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks,
            lock_policy,
            secret_store,
            shadow_store,
            event_sink: Arc::new(NoopEventSink),
        }
    }

    pub fn with_event_sink(mut self, event_sink: Arc<dyn EventSink>) -> Self {
        self.event_sink = event_sink;
        self
    }

    async fn dry_run_plan(&self, request: &ApplyIntentRequest) -> UnderlayResult<DryRunPlan> {
        validate_switch_pair_intent(&request.intent)?;

        let desired_states = plan_switch_pair(&request.intent);
        self.dry_run_desired_states(&desired_states).await
    }

    async fn dry_run_domain_plan(
        &self,
        request: &ApplyDomainIntentRequest,
    ) -> UnderlayResult<DryRunPlan> {
        let desired_states = plan_underlay_domain(&request.intent)?;
        self.dry_run_desired_states(&desired_states).await
    }

    pub async fn apply_domain_intent(
        &self,
        request: ApplyDomainIntentRequest,
    ) -> UnderlayResult<ApplyIntentResponse> {
        let request_id = request.request_id.clone();
        let trace_id = request
            .trace_id
            .clone()
            .unwrap_or_else(|| request_id.clone());
        let desired_states = plan_underlay_domain(&request.intent)?;
        self.apply_desired_states(
            request_id,
            trace_id,
            desired_states,
            request.options.allow_degraded_atomicity,
            request.options.drift_policy,
        )
        .await
    }

    pub async fn dry_run_domain(
        &self,
        request: ApplyDomainIntentRequest,
    ) -> UnderlayResult<DryRunResponse> {
        let plan = self.dry_run_domain_plan(&request).await?;
        Ok(DryRunResponse {
            device_results: device_results_from_plan(&plan),
            noop: plan.is_noop(),
            change_sets: plan.change_sets,
        })
    }

    async fn fetch_current_states(
        &self,
        desired_states: &[DeviceDesiredState],
    ) -> UnderlayResult<Vec<DeviceShadowState>> {
        let mut current_states = Vec::with_capacity(desired_states.len());

        for desired in desired_states {
            let managed = self.inventory.get(&desired.device_id)?;
            let mut client = AdapterClient::connect(managed.info.adapter_endpoint.clone()).await?;
            // Preflight diff needs an authoritative current view. If we scope this
            // only to desired objects, absent desired resources cannot be detected
            // as deletes. Post-commit verify is scoped by ChangeSet below.
            let current = client.get_current_state(&managed.info).await?;
            self.shadow_store.put(current.clone())?;
            current_states.push(current);
        }

        Ok(current_states)
    }

    async fn fetch_device_state_from_adapter(
        &self,
        device_id: &DeviceId,
    ) -> UnderlayResult<DeviceShadowState> {
        let managed = self.inventory.get(device_id)?;
        let mut client = AdapterClient::connect(managed.info.adapter_endpoint.clone()).await?;
        client.get_current_state(&managed.info).await
    }

    async fn dry_run_desired_states(
        &self,
        desired_states: &[DeviceDesiredState],
    ) -> UnderlayResult<DryRunPlan> {
        let current_states = self.fetch_current_states(desired_states).await?;
        build_dry_run_plan(desired_states, &current_states)
    }

    async fn apply_desired_states(
        &self,
        request_id: String,
        trace_id: String,
        desired_states: Vec<DeviceDesiredState>,
        allow_degraded_atomicity: bool,
        drift_policy: DriftPolicy,
    ) -> UnderlayResult<ApplyIntentResponse> {
        let mut device_results = Vec::with_capacity(desired_states.len());

        for desired in &desired_states {
            device_results.push(
                self.apply_single_endpoint_state(
                    &request_id,
                    &trace_id,
                    desired,
                    allow_degraded_atomicity,
                    drift_policy,
                )
                .await,
            );
        }

        self.emit_apply_events(&request_id, &trace_id, &device_results);

        let status = aggregate_apply_status(&device_results);
        let tx_id = if device_results.len() == 1 {
            device_results[0].tx_id.clone()
        } else {
            None
        };
        let strategy = if device_results.len() == 1 {
            device_results[0].strategy
        } else {
            None
        };
        let warnings = device_results
            .iter()
            .flat_map(|result| result.warnings.clone())
            .collect();

        Ok(ApplyIntentResponse {
            request_id,
            trace_id,
            tx_id,
            status,
            strategy,
            device_results,
            warnings,
        })
    }

    fn emit_apply_events(
        &self,
        request_id: &str,
        trace_id: &str,
        device_results: &[DeviceApplyResult],
    ) {
        for result in device_results {
            if let Some(event) =
                UnderlayEvent::from_device_apply_result(request_id, trace_id, result)
            {
                self.event_sink.emit(event);
            }
        }
    }

    async fn apply_single_endpoint_state(
        &self,
        request_id: &str,
        trace_id: &str,
        desired: &DeviceDesiredState,
        allow_degraded_atomicity: bool,
        drift_policy: DriftPolicy,
    ) -> DeviceApplyResult {
        let _endpoint_guard = match self
            .endpoint_locks
            .acquire_many_with_policy(std::slice::from_ref(&desired.device_id), &self.lock_policy)
            .await
        {
            Ok(guard) => guard,
            Err(err) => return device_error_result(&desired.device_id, false, None, None, err),
        };

        if let Err(err) =
            self.ensure_no_in_doubt_for_devices(std::slice::from_ref(&desired.device_id))
        {
            return device_error_result(&desired.device_id, false, None, None, err);
        }
        if let Err(err) = self.ensure_drift_policy_allows_apply(
            std::slice::from_ref(&desired.device_id),
            drift_policy,
        ) {
            return device_error_result(&desired.device_id, false, None, None, err);
        }

        let plan = match self
            .dry_run_desired_states(std::slice::from_ref(desired))
            .await
        {
            Ok(plan) => plan,
            Err(err) => return device_error_result(&desired.device_id, false, None, None, err),
        };
        let changed = !plan.is_noop();

        if !changed {
            return DeviceApplyResult {
                device_id: desired.device_id.clone(),
                changed: false,
                status: ApplyStatus::NoOpSuccess,
                tx_id: None,
                strategy: None,
                error_code: None,
                error_message: None,
                warnings: Vec::new(),
            };
        }

        let tx_context = TxContext::new(request_id.to_string(), trace_id.to_string());
        let mut journal_record =
            TxJournalRecord::started(&tx_context, vec![desired.device_id.clone()]);
        if let Err(err) = self.journal.put(&journal_record) {
            return device_error_result(&desired.device_id, true, None, None, err);
        }
        journal_record = journal_record.with_phase(TxPhase::Preparing);
        if let Err(err) = self.journal.put(&journal_record) {
            return device_error_result(
                &desired.device_id,
                true,
                Some(tx_context.tx_id),
                None,
                err,
            );
        }

        match self
            .apply_changed_endpoint_states(
                std::slice::from_ref(desired),
                &plan,
                &tx_context,
                &mut journal_record,
                allow_degraded_atomicity,
            )
            .await
        {
            Ok(strategy) => {
                let mut warnings = degraded_strategy_warnings(strategy);
                let shadow_state = DeviceShadowState::from_desired(desired, 0);
                if let Err(err) = self.shadow_store.put(shadow_state) {
                    let (code, message) = journal_error_fields(&err);
                    let error_message =
                        format!("shadow state stale after successful apply: {message}");
                    journal_record = journal_record
                        .with_strategy(strategy)
                        .with_phase(TxPhase::InDoubt)
                        .with_error(code.clone(), error_message.clone());
                    if let Err(journal_err) = self.journal.put(&journal_record) {
                        let (_, journal_msg) = journal_error_fields(&journal_err);
                        return DeviceApplyResult {
                            device_id: desired.device_id.clone(),
                            changed: true,
                            status: ApplyStatus::InDoubt,
                            tx_id: Some(tx_context.tx_id),
                            strategy: Some(strategy),
                            error_code: Some(code),
                            error_message: Some(format!(
                                "{error_message}; journal write also failed: {journal_msg}"
                            )),
                            warnings,
                        };
                    }
                    warnings.push(format!("shadow state update failed: {message}"));
                    return DeviceApplyResult {
                        device_id: desired.device_id.clone(),
                        changed: true,
                        status: ApplyStatus::InDoubt,
                        tx_id: Some(tx_context.tx_id),
                        strategy: Some(strategy),
                        error_code: Some(code),
                        error_message: Some(error_message),
                        warnings,
                    };
                }
                journal_record = journal_record
                    .with_strategy(strategy)
                    .with_phase(TxPhase::Committed);
                if let Err(err) = self.journal.put(&journal_record) {
                    let (code, message) = journal_error_fields(&err);
                    return DeviceApplyResult {
                        device_id: desired.device_id.clone(),
                        changed: true,
                        status: ApplyStatus::InDoubt,
                        tx_id: Some(tx_context.tx_id),
                        strategy: Some(strategy),
                        error_code: Some(code),
                        error_message: Some(format!(
                            "adapter committed and shadow was updated, \
                             but terminal journal write failed: {message}"
                        )),
                        warnings,
                    };
                }
                DeviceApplyResult {
                    device_id: desired.device_id.clone(),
                    changed: true,
                    status: if strategy.is_degraded() {
                        ApplyStatus::SuccessWithWarning
                    } else {
                        ApplyStatus::Success
                    },
                    tx_id: Some(tx_context.tx_id),
                    strategy: Some(strategy),
                    error_code: None,
                    error_message: None,
                    warnings,
                }
            }
            Err(err) => {
                let (code, message) = journal_error_fields(&err);
                let phase = failed_apply_phase(&journal_record.phase);
                journal_record = journal_record
                    .with_phase(phase.clone())
                    .with_error(code.clone(), message.clone());
                if let Err(journal_err) = self.journal.put(&journal_record) {
                    let (_, journal_msg) = journal_error_fields(&journal_err);
                    return DeviceApplyResult {
                        device_id: desired.device_id.clone(),
                        changed: true,
                        status: apply_status_for_failed_phase(&phase),
                        tx_id: Some(tx_context.tx_id),
                        strategy: journal_record.strategy,
                        error_code: Some(code),
                        error_message: Some(format!(
                            "{message}; journal write also failed: {journal_msg}"
                        )),
                        warnings: Vec::new(),
                    };
                }
                DeviceApplyResult {
                    device_id: desired.device_id.clone(),
                    changed: true,
                    status: apply_status_for_failed_phase(&phase),
                    tx_id: Some(tx_context.tx_id),
                    strategy: journal_record.strategy,
                    error_code: Some(code),
                    error_message: Some(message),
                    warnings: Vec::new(),
                }
            }
        }
    }

    async fn apply_changed_endpoint_states(
        &self,
        desired_states: &[DeviceDesiredState],
        plan: &DryRunPlan,
        tx_context: &TxContext,
        journal_record: &mut TxJournalRecord,
        allow_degraded_atomicity: bool,
    ) -> UnderlayResult<TransactionStrategy> {
        let mut selected_strategy = None;

        for desired in desired_states {
            let Some(change_set) = plan
                .change_sets
                .iter()
                .find(|change_set| change_set.device_id == desired.device_id)
            else {
                return Err(UnderlayError::InvalidDeviceState(format!(
                    "missing change set for endpoint {}",
                    desired.device_id.0
                )));
            };
            if change_set.is_empty() {
                continue;
            }

            let managed = self.inventory.get(&desired.device_id)?;
            let mut client = AdapterClient::connect(managed.info.adapter_endpoint.clone()).await?;
            let rpc_context = tx_request_context(&managed.info, tx_context);
            let capability = match managed.capability {
                Some(capability) => capability,
                None => client.get_capabilities(&managed.info).await?,
            };
            let strategy = capability.recommended_strategy;
            if !strategy.is_supported() {
                return Err(UnderlayError::UnsupportedTransactionStrategy);
            }
            if strategy.is_degraded() && !allow_degraded_atomicity {
                return Err(UnderlayError::AdapterOperation {
                    code: "DEGRADED_ATOMICITY_NOT_ALLOWED".into(),
                    message: format!(
                        "device {} requires degraded transaction strategy {:?}, but request disallows degraded atomicity",
                        desired.device_id.0, strategy
                    ),
                    retryable: false,
                    errors: Vec::new(),
                });
            }
            selected_strategy.get_or_insert(strategy);
            *journal_record = journal_record.clone().with_strategy(strategy);
            self.journal.put(journal_record)?;

            let prepare = match client
                .prepare_with_context(&managed.info, &rpc_context, desired)
                .await
            {
                Ok(prepare) => prepare,
                Err(err) => {
                    self.rollback_after_endpoint_failure(
                        &mut client,
                        &managed.info,
                        &rpc_context,
                        journal_record,
                    )
                    .await?;
                    return Err(err);
                }
            };
            if prepare.status != AdapterOperationStatus::Prepared {
                self.rollback_after_endpoint_failure(
                    &mut client,
                    &managed.info,
                    &rpc_context,
                    journal_record,
                )
                .await?;
                return Err(UnderlayError::AdapterOperation {
                    code: "UNEXPECTED_PREPARE_STATUS".into(),
                    message: format!("adapter returned prepare status {:?}", prepare.status),
                    retryable: false,
                    errors: Vec::new(),
                });
            }
            *journal_record = journal_record.clone().with_phase(TxPhase::Prepared);
            self.journal.put(journal_record)?;

            *journal_record = journal_record.clone().with_phase(TxPhase::Committing);
            self.journal.put(journal_record)?;
            match client
                .commit_with_context(&managed.info, &rpc_context, strategy)
                .await
            {
                Ok(commit) if commit_status_matches_strategy(commit.status, strategy) => {}
                Ok(commit) => {
                    self.rollback_after_endpoint_failure(
                        &mut client,
                        &managed.info,
                        &rpc_context,
                        journal_record,
                    )
                    .await?;
                    return Err(UnderlayError::AdapterOperation {
                        code: "UNEXPECTED_COMMIT_STATUS".into(),
                        message: format!("adapter returned commit status {:?}", commit.status),
                        retryable: false,
                        errors: Vec::new(),
                    });
                }
                Err(err) => {
                    self.rollback_after_endpoint_failure(
                        &mut client,
                        &managed.info,
                        &rpc_context,
                        journal_record,
                    )
                    .await?;
                    return Err(err);
                }
            }

            *journal_record = journal_record.clone().with_phase(TxPhase::Verifying);
            self.journal.put(journal_record)?;
            match client
                .verify_with_context_for_change_set(
                    &managed.info,
                    &rpc_context,
                    desired,
                    change_set,
                )
                .await
            {
                Ok(verify)
                    if matches!(
                        verify.status,
                        AdapterOperationStatus::NoChange | AdapterOperationStatus::Committed
                    ) => {}
                Ok(verify) => {
                    self.rollback_after_endpoint_failure(
                        &mut client,
                        &managed.info,
                        &rpc_context,
                        journal_record,
                    )
                    .await?;
                    return Err(UnderlayError::AdapterOperation {
                        code: "UNEXPECTED_VERIFY_STATUS".into(),
                        message: format!("adapter returned verify status {:?}", verify.status),
                        retryable: false,
                        errors: Vec::new(),
                    });
                }
                Err(err) => {
                    self.rollback_after_endpoint_failure(
                        &mut client,
                        &managed.info,
                        &rpc_context,
                        journal_record,
                    )
                    .await?;
                    return Err(err);
                }
            }

            if strategy == TransactionStrategy::ConfirmedCommit {
                *journal_record = journal_record.clone().with_phase(TxPhase::FinalConfirming);
                self.journal.put(journal_record)?;
                match client
                    .final_confirm_with_context(&managed.info, &rpc_context)
                    .await
                {
                    Ok(confirm) if confirm.status == AdapterOperationStatus::Committed => {}
                    Ok(confirm) => {
                        self.rollback_after_endpoint_failure(
                            &mut client,
                            &managed.info,
                            &rpc_context,
                            journal_record,
                        )
                        .await?;
                        return Err(UnderlayError::AdapterOperation {
                            code: "UNEXPECTED_FINAL_CONFIRM_STATUS".into(),
                            message: format!(
                                "adapter returned final confirm status {:?}",
                                confirm.status
                            ),
                            retryable: false,
                            errors: Vec::new(),
                        });
                    }
                    Err(err) => {
                        self.rollback_after_endpoint_failure(
                            &mut client,
                            &managed.info,
                            &rpc_context,
                            journal_record,
                        )
                        .await?;
                        return Err(err);
                    }
                }
            }
        }

        selected_strategy.ok_or(UnderlayError::UnsupportedTransactionStrategy)
    }

    async fn rollback_after_endpoint_failure(
        &self,
        client: &mut AdapterClient,
        device: &DeviceInfo,
        context: &RequestContext,
        journal_record: &mut TxJournalRecord,
    ) -> UnderlayResult<()> {
        *journal_record = journal_record.clone().with_phase(TxPhase::RollingBack);
        self.journal.put(journal_record)?;

        match client
            .rollback_with_context(device, context, journal_record.strategy)
            .await
        {
            Ok(outcome)
                if matches!(
                    outcome.status,
                    AdapterOperationStatus::RolledBack | AdapterOperationStatus::NoChange
                ) =>
            {
                *journal_record = journal_record.clone().with_phase(TxPhase::RolledBack);
                self.journal.put(journal_record)?;
            }
            Ok(outcome) => {
                *journal_record = journal_record
                    .clone()
                    .with_phase(TxPhase::InDoubt)
                    .with_error(
                        "UNEXPECTED_ROLLBACK_STATUS",
                        format!("adapter returned rollback status {:?}", outcome.status),
                    );
                self.journal.put(journal_record)?;
            }
            Err(err) => {
                let (code, message) = journal_error_fields(&err);
                *journal_record = journal_record
                    .clone()
                    .with_phase(TxPhase::InDoubt)
                    .with_error(code, message);
                self.journal.put(journal_record)?;
            }
        }

        Ok(())
    }

    fn ensure_no_in_doubt_for_devices(&self, device_ids: &[DeviceId]) -> UnderlayResult<()> {
        let recoverable = self.journal.list_recoverable()?;
        let blocking = in_doubt_records_for_devices(&recoverable, device_ids);
        if blocking.is_empty() {
            return Ok(());
        }

        let tx_ids = blocking
            .iter()
            .map(|record| record.tx_id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        Err(UnderlayError::AdapterOperation {
            code: "TX_IN_DOUBT".into(),
            message: format!("endpoint has unresolved in-doubt transaction(s): {tx_ids}"),
            retryable: false,
            errors: Vec::new(),
        })
    }

    fn ensure_drift_policy_allows_apply(
        &self,
        device_ids: &[DeviceId],
        policy: DriftPolicy,
    ) -> UnderlayResult<()> {
        if policy == DriftPolicy::ReportOnly {
            return Ok(());
        }

        let drifted_devices = device_ids
            .iter()
            .filter_map(|device_id| match self.inventory.get(device_id) {
                Ok(managed) if managed.info.lifecycle_state == DeviceLifecycleState::Drifted => {
                    Some(Ok(device_id.0.clone()))
                }
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            })
            .collect::<UnderlayResult<Vec<_>>>()?;

        if drifted_devices.is_empty() {
            return Ok(());
        }

        let device_list = drifted_devices.join(",");
        match policy {
            DriftPolicy::BlockNewTransaction => Err(UnderlayError::AdapterOperation {
                code: "DRIFT_BLOCKED".into(),
                message: format!(
                    "device has unresolved out-of-band drift: {device_list}"
                ),
                retryable: false,
                errors: Vec::new(),
            }),
            DriftPolicy::AutoReconcile => Err(UnderlayError::AdapterOperation {
                code: "DRIFT_AUTORECONCILE_UNIMPLEMENTED".into(),
                message: format!(
                    "auto reconcile is not implemented for drifted device(s): {device_list}"
                ),
                retryable: false,
                errors: Vec::new(),
            }),
            DriftPolicy::ReportOnly => Ok(()),
        }
    }

    async fn recover_record(
        &self,
        record: &TxJournalRecord,
        action: RecoveryAction,
    ) -> UnderlayResult<TxPhase> {
        if record.devices.is_empty() {
            return Ok(match action {
                RecoveryAction::DiscardPreparedChanges => TxPhase::RolledBack,
                _ => TxPhase::InDoubt,
            });
        }

        let tx_context = TxContext {
            tx_id: record.tx_id.clone(),
            request_id: record.request_id.clone(),
            trace_id: record.trace_id.clone(),
        };
        let mut merged_phase = None;

        for device_id in &record.devices {
            let managed = self.inventory.get(device_id)?;
            let mut client = AdapterClient::connect(managed.info.adapter_endpoint.clone()).await?;
            let rpc_context = tx_request_context(&managed.info, &tx_context);
            let outcome = client
                .recover_with_context(&managed.info, &rpc_context, record.strategy, action)
                .await?;
            let phase = recover_phase_from_adapter_status(action, outcome.status);
            merged_phase = Some(merge_recovery_phase(merged_phase, phase));
        }

        Ok(merged_phase.unwrap_or(TxPhase::InDoubt))
    }
}

fn recover_phase_from_adapter_status(
    action: RecoveryAction,
    status: AdapterOperationStatus,
) -> TxPhase {
    match (action, status) {
        (_, AdapterOperationStatus::RolledBack) => TxPhase::RolledBack,
        (RecoveryAction::AdapterRecover, AdapterOperationStatus::Committed) => TxPhase::Committed,
        (RecoveryAction::DiscardPreparedChanges, AdapterOperationStatus::NoChange) => {
            TxPhase::RolledBack
        }
        _ => TxPhase::InDoubt,
    }
}

fn commit_status_matches_strategy(
    status: AdapterOperationStatus,
    strategy: TransactionStrategy,
) -> bool {
    match strategy {
        TransactionStrategy::ConfirmedCommit => {
            status == AdapterOperationStatus::ConfirmedCommitPending
        }
        _ => status == AdapterOperationStatus::Committed,
    }
}

fn merge_recovery_phase(current: Option<TxPhase>, next: TxPhase) -> TxPhase {
    match (current, next) {
        (None, phase) => phase,
        (Some(left), right) if left == right => left,
        (Some(TxPhase::InDoubt), _) | (_, TxPhase::InDoubt) => TxPhase::InDoubt,
        _ => TxPhase::InDoubt,
    }
}

fn failed_apply_phase(current: &TxPhase) -> TxPhase {
    match current {
        TxPhase::RolledBack | TxPhase::InDoubt => current.clone(),
        _ => TxPhase::Failed,
    }
}

#[async_trait]
impl UnderlayService for AriaUnderlayService {
    async fn initialize_underlay_site(
        &self,
        request: InitializeUnderlaySiteRequest,
    ) -> UnderlayResult<InitializeUnderlaySiteResponse> {
        UnderlaySiteInitializationService::new_with_secret_store(
            self.inventory.clone(),
            self.secret_store.clone(),
        )
        .initialize_site(request)
        .await
    }

    async fn register_device(
        &self,
        request: RegisterDeviceRequest,
    ) -> UnderlayResult<RegisterDeviceResponse> {
        DeviceRegistrationService::new(self.inventory.clone()).register(request)
    }

    async fn onboard_device(
        &self,
        device_id: DeviceId,
    ) -> UnderlayResult<DeviceOnboardingResponse> {
        let lifecycle_state = DeviceOnboardingService::new(self.inventory.clone())
            .onboard_device(device_id.clone())
            .await?;
        Ok(DeviceOnboardingResponse {
            device_id,
            lifecycle_state,
        })
    }

    async fn apply_intent(
        &self,
        request: ApplyIntentRequest,
    ) -> UnderlayResult<ApplyIntentResponse> {
        validate_switch_pair_intent(&request.intent)?;
        let request_id = request.request_id.clone();
        let trace_id = request
            .trace_id
            .clone()
            .unwrap_or_else(|| request_id.clone());
        let desired_states = plan_switch_pair(&request.intent);
        self.apply_desired_states(
            request_id,
            trace_id,
            desired_states,
            request.options.allow_degraded_atomicity,
            request.options.drift_policy,
        )
        .await
    }

    async fn dry_run(&self, request: ApplyIntentRequest) -> UnderlayResult<DryRunResponse> {
        let plan = self.dry_run_plan(&request).await?;
        Ok(DryRunResponse {
            device_results: device_results_from_plan(&plan),
            noop: plan.is_noop(),
            change_sets: plan.change_sets,
        })
    }

    async fn refresh_state(
        &self,
        request: RefreshStateRequest,
    ) -> UnderlayResult<RefreshStateResponse> {
        for device_id in &request.device_ids {
            self.get_device_state(device_id.clone()).await?;
        }
        Ok(RefreshStateResponse {
            refreshed_devices: request.device_ids,
        })
    }

    async fn get_device_state(&self, device_id: DeviceId) -> UnderlayResult<DeviceShadowState> {
        let state = self.fetch_device_state_from_adapter(&device_id).await?;
        self.shadow_store.put(state.clone())?;
        Ok(state)
    }

    async fn recover_pending_transactions(&self) -> UnderlayResult<RecoveryReport> {
        let recoverable = self.journal.list_recoverable()?;
        let mut decisions = Vec::with_capacity(recoverable.len());
        let mut recovered = 0;

        for candidate in recoverable {
            let _endpoint_guard = self.endpoint_locks.acquire_many(&candidate.devices).await?;
            let Some(record) = self.journal.get(&candidate.tx_id)? else {
                continue;
            };
            let decision = classify_recovery(&record);
            decisions.push(decision.clone());
            if !record.phase.requires_recovery() {
                continue;
            }

            match decision.action {
                RecoveryAction::Noop => {}
                RecoveryAction::ManualIntervention => {
                    if record.phase != TxPhase::InDoubt {
                        self.journal
                            .put(&record.clone().with_phase(TxPhase::InDoubt))?;
                    }
                }
                RecoveryAction::DiscardPreparedChanges | RecoveryAction::AdapterRecover => {
                    self.journal
                        .put(&record.clone().with_phase(TxPhase::Recovering))?;

                    match self.recover_record(&record, decision.action).await {
                        Ok(phase) => {
                            let terminal =
                                matches!(phase, TxPhase::Committed | TxPhase::RolledBack);
                            self.journal.put(&record.clone().with_phase(phase))?;
                            if terminal {
                                recovered += 1;
                            }
                        }
                        Err(err) => {
                            let (code, message) = journal_error_fields(&err);
                            self.journal.put(
                                &record
                                    .clone()
                                    .with_phase(TxPhase::InDoubt)
                                    .with_error(code, message),
                            )?;
                        }
                    }
                }
            }
        }

        let pending_records = self.journal.list_recoverable()?;
        let in_doubt = pending_records
            .iter()
            .filter(|record| record.phase == TxPhase::InDoubt)
            .count();
        let tx_ids = pending_records
            .iter()
            .map(|record| record.tx_id.clone())
            .collect::<Vec<_>>();

        Ok(RecoveryReport {
            recovered,
            in_doubt,
            pending: pending_records.len(),
            tx_ids,
            decisions,
        })
    }

    async fn run_drift_audit(
        &self,
        request: DriftAuditRequest,
    ) -> UnderlayResult<DriftAuditResponse> {
        let mut drifted_devices = Vec::new();

        for device_id in request.device_ids {
            let observed = self.fetch_device_state_from_adapter(&device_id).await?;
            let report = match self.shadow_store.get(&device_id)? {
                Some(expected) => detect_drift(&expected, &observed),
                None => DriftReport::from_adapter_warnings(
                    device_id.clone(),
                    observed.warnings.clone(),
                ),
            };
            if report.drift_detected {
                self.event_sink.emit(UnderlayEvent::drift_detected(
                    "drift-audit",
                    "drift-audit",
                    &report,
                ));
                self.inventory
                    .set_state(&device_id, crate::device::DeviceLifecycleState::Drifted)?;
                drifted_devices.push(device_id);
            } else {
                self.shadow_store.put(observed)?;
            }
        }

        Ok(DriftAuditResponse { drifted_devices })
    }

    async fn force_unlock(
        &self,
        request: ForceUnlockRequest,
    ) -> UnderlayResult<ForceUnlockResponse> {
        let managed = self.inventory.get(&request.device_id)?;
        let mut client = AdapterClient::connect(managed.info.adapter_endpoint.clone()).await?;
        let context = RequestContext {
            request_id: request.request_id.clone(),
            tx_id: String::new(),
            trace_id: request
                .trace_id
                .clone()
                .unwrap_or_else(|| request.request_id.clone()),
            tenant_id: managed.info.tenant_id.clone(),
            site_id: managed.info.site_id.clone(),
        };
        let outcome = client
            .force_unlock(
                &managed.info,
                &context,
                request.lock_owner,
                request.reason,
                request.break_glass_enabled,
            )
            .await?;

        Ok(ForceUnlockResponse {
            device_id: request.device_id,
            unlocked: matches!(outcome.status, AdapterOperationStatus::Committed),
            warnings: outcome.warnings,
        })
    }

    async fn force_resolve_transaction(
        &self,
        request: ForceResolveTransactionRequest,
    ) -> UnderlayResult<ForceResolveTransactionResponse> {
        validate_force_resolve_request(&request)?;
        let trace_id = request
            .trace_id
            .clone()
            .unwrap_or_else(|| request.request_id.clone());

        let Some(candidate) = self.journal.get(&request.tx_id)? else {
            return Err(UnderlayError::AdapterOperation {
                code: "TX_NOT_FOUND".into(),
                message: format!("transaction {} was not found", request.tx_id),
                retryable: false,
                errors: Vec::new(),
            });
        };

        let _endpoint_guard = self.endpoint_locks.acquire_many(&candidate.devices).await?;
        let Some(record) = self.journal.get(&request.tx_id)? else {
            return Err(UnderlayError::AdapterOperation {
                code: "TX_NOT_FOUND".into(),
                message: format!("transaction {} was not found", request.tx_id),
                retryable: false,
                errors: Vec::new(),
            });
        };
        if record.phase != TxPhase::InDoubt {
            return Err(UnderlayError::AdapterOperation {
                code: "TX_NOT_IN_DOUBT".into(),
                message: format!(
                    "transaction {} is {:?}, not InDoubt",
                    record.tx_id, record.phase
                ),
                retryable: false,
                errors: Vec::new(),
            });
        }

        let previous_phase = record.phase.clone();
        let resolved = record
            .clone()
            .with_manual_resolution(
                request.operator.clone(),
                request.reason.clone(),
                request.request_id.clone(),
                trace_id.clone(),
            )
            .with_phase(TxPhase::ForceResolved);
        self.journal.put(&resolved)?;
        self.event_sink.emit(UnderlayEvent::transaction_force_resolved(
            request.request_id,
            trace_id,
            record.tx_id.clone(),
            previous_phase.clone(),
            &record.devices,
            request.operator,
            request.reason,
        ));

        Ok(ForceResolveTransactionResponse {
            tx_id: record.tx_id,
            previous_phase,
            resolved_phase: TxPhase::ForceResolved,
            devices: record.devices,
            resolved: true,
            warnings: Vec::new(),
        })
    }
}

fn device_results_from_plan(plan: &DryRunPlan) -> Vec<DeviceApplyResult> {
    plan.change_sets
        .iter()
        .map(|change_set| DeviceApplyResult {
            device_id: change_set.device_id.clone(),
            changed: !change_set.is_empty(),
            status: if change_set.is_empty() {
                ApplyStatus::NoOpSuccess
            } else {
                ApplyStatus::Success
            },
            tx_id: None,
            strategy: None,
            error_code: None,
            error_message: None,
            warnings: Vec::new(),
        })
        .collect()
}

fn validate_force_resolve_request(request: &ForceResolveTransactionRequest) -> UnderlayResult<()> {
    if request.tx_id.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "force resolve requires tx_id".into(),
        ));
    }
    if request.operator.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "force resolve requires operator".into(),
        ));
    }
    if request.reason.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "force resolve requires reason".into(),
        ));
    }
    if !request.break_glass_enabled {
        return Err(UnderlayError::AdapterOperation {
            code: "FORCE_RESOLVE_BREAK_GLASS_REQUIRED".into(),
            message: "break-glass must be enabled to force resolve an in-doubt transaction"
                .into(),
            retryable: false,
            errors: Vec::new(),
        });
    }

    Ok(())
}

fn aggregate_apply_status(device_results: &[DeviceApplyResult]) -> ApplyStatus {
    if device_results.is_empty() {
        ApplyStatus::Failed
    } else if device_results
        .iter()
        .all(|result| result.status == ApplyStatus::NoOpSuccess)
    {
        ApplyStatus::NoOpSuccess
    } else if device_results
        .iter()
        .any(|result| result.status == ApplyStatus::InDoubt)
    {
        ApplyStatus::InDoubt
    } else if device_results
        .iter()
        .all(|result| {
            matches!(
                result.status,
                ApplyStatus::Success | ApplyStatus::SuccessWithWarning | ApplyStatus::NoOpSuccess
            )
        })
    {
        if device_results
            .iter()
            .any(|result| result.status == ApplyStatus::SuccessWithWarning)
        {
            ApplyStatus::SuccessWithWarning
        } else {
            ApplyStatus::Success
        }
    } else if device_results
        .iter()
        .all(|result| result.status == ApplyStatus::RolledBack)
    {
        ApplyStatus::RolledBack
    } else if device_results.len() == 1 {
        device_results[0].status.clone()
    } else {
        ApplyStatus::Failed
    }
}

fn device_error_result(
    device_id: &DeviceId,
    changed: bool,
    tx_id: Option<String>,
    strategy: Option<TransactionStrategy>,
    error: UnderlayError,
) -> DeviceApplyResult {
    let (code, message) = journal_error_fields(&error);
    DeviceApplyResult {
        device_id: device_id.clone(),
        changed,
        status: if code == "TX_IN_DOUBT" {
            ApplyStatus::InDoubt
        } else {
            ApplyStatus::Failed
        },
        tx_id,
        strategy,
        error_code: Some(code),
        error_message: Some(message),
        warnings: Vec::new(),
    }
}

fn apply_status_for_failed_phase(phase: &TxPhase) -> ApplyStatus {
    match phase {
        TxPhase::RolledBack => ApplyStatus::RolledBack,
        TxPhase::InDoubt => ApplyStatus::InDoubt,
        _ => ApplyStatus::Failed,
    }
}

fn degraded_strategy_warnings(strategy: TransactionStrategy) -> Vec<String> {
    if strategy.is_degraded() {
        vec![format!(
            "degraded transaction strategy {:?}; atomicity is weaker than confirmed commit",
            strategy
        )]
    } else {
        Vec::new()
    }
}

fn journal_error_fields(error: &UnderlayError) -> (String, String) {
    match error {
        UnderlayError::AdapterOperation { code, message, .. } => (code.clone(), message.clone()),
        UnderlayError::AdapterTransport(message) => {
            ("ADAPTER_TRANSPORT".into(), message.clone())
        }
        UnderlayError::InvalidIntent(message) => ("INVALID_INTENT".into(), message.clone()),
        UnderlayError::InvalidDeviceState(message) => {
            ("INVALID_DEVICE_STATE".into(), message.clone())
        }
        UnderlayError::UnsupportedTransactionStrategy => (
            "UNSUPPORTED_TRANSACTION_STRATEGY".into(),
            "unsupported transaction strategy".into(),
        ),
        UnderlayError::DeviceAlreadyExists(device_id) => {
            ("DEVICE_ALREADY_EXISTS".into(), device_id.clone())
        }
        UnderlayError::DeviceNotFound(device_id) => ("DEVICE_NOT_FOUND".into(), device_id.clone()),
        UnderlayError::Internal(message) => ("INTERNAL".into(), message.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::HostKeyPolicy;
    use crate::model::{DeviceRole, Vendor};

    fn result(status: ApplyStatus) -> DeviceApplyResult {
        DeviceApplyResult {
            device_id: DeviceId("leaf-a".into()),
            changed: status != ApplyStatus::NoOpSuccess,
            status,
            tx_id: None,
            strategy: None,
            error_code: None,
            error_message: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn aggregate_empty_results_are_failed() {
        let status = aggregate_apply_status(&[]);

        assert_eq!(status, ApplyStatus::Failed);
    }

    #[test]
    fn aggregate_partial_failure_is_failed() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::Success),
            result(ApplyStatus::Failed),
        ]);

        assert_eq!(status, ApplyStatus::Failed);
    }

    #[test]
    fn aggregate_partial_rollback_is_failed() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::Success),
            result(ApplyStatus::RolledBack),
        ]);

        assert_eq!(status, ApplyStatus::Failed);
    }

    #[test]
    fn aggregate_success_with_warning_is_successful_but_warns() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::SuccessWithWarning),
            result(ApplyStatus::Success),
        ]);

        assert_eq!(status, ApplyStatus::SuccessWithWarning);
    }

    #[test]
    fn aggregate_all_degraded_successes_do_not_become_failed() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::SuccessWithWarning),
            result(ApplyStatus::SuccessWithWarning),
        ]);

        assert_eq!(status, ApplyStatus::SuccessWithWarning);
    }

    #[test]
    fn degraded_strategy_warning_only_for_degraded_strategies() {
        assert!(degraded_strategy_warnings(TransactionStrategy::ConfirmedCommit).is_empty());
        assert_eq!(
            degraded_strategy_warnings(TransactionStrategy::RunningRollbackOnError).len(),
            1
        );
    }

    #[test]
    fn report_only_drift_policy_does_not_block_drifted_device() {
        let service = service_with_device_state(DeviceLifecycleState::Drifted);

        service
            .ensure_drift_policy_allows_apply(
                &[DeviceId("leaf-a".into())],
                DriftPolicy::ReportOnly,
            )
            .expect("report-only drift policy should not block apply");
    }

    #[test]
    fn block_new_transaction_policy_blocks_drifted_device() {
        let service = service_with_device_state(DeviceLifecycleState::Drifted);

        let err = service
            .ensure_drift_policy_allows_apply(
                &[DeviceId("leaf-a".into())],
                DriftPolicy::BlockNewTransaction,
            )
            .expect_err("block-new-transaction should reject drifted device");

        match err {
            UnderlayError::AdapterOperation { code, retryable, .. } => {
                assert_eq!(code, "DRIFT_BLOCKED");
                assert!(!retryable);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn auto_reconcile_policy_fails_closed_when_drifted_device_exists() {
        let service = service_with_device_state(DeviceLifecycleState::Drifted);

        let err = service
            .ensure_drift_policy_allows_apply(
                &[DeviceId("leaf-a".into())],
                DriftPolicy::AutoReconcile,
            )
            .expect_err("auto-reconcile is not implemented yet");

        match err {
            UnderlayError::AdapterOperation { code, retryable, .. } => {
                assert_eq!(code, "DRIFT_AUTORECONCILE_UNIMPLEMENTED");
                assert!(!retryable);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn service_with_device_state(state: DeviceLifecycleState) -> AriaUnderlayService {
        let inventory = DeviceInventory::default();
        inventory
            .insert(DeviceInfo {
                tenant_id: "tenant-a".into(),
                site_id: "site-a".into(),
                id: DeviceId("leaf-a".into()),
                management_ip: "127.0.0.1".into(),
                management_port: 830,
                vendor_hint: Some(Vendor::Unknown),
                model_hint: None,
                role: DeviceRole::LeafA,
                secret_ref: "local/tenant-a/site-a/leaf-a".into(),
                host_key_policy: HostKeyPolicy::TrustOnFirstUse,
                adapter_endpoint: "http://127.0.0.1:50051".into(),
                lifecycle_state: state,
            })
            .expect("device insert should succeed");
        AriaUnderlayService::new(inventory)
    }
}
