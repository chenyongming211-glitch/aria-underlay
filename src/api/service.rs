use async_trait::async_trait;
use std::sync::Arc;

use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::adapter_client::{tx_request_context, AdapterClient};
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
    DeviceInfo, DeviceInventory, DeviceOnboardingService, DeviceRegistrationService,
    InMemorySecretStore, InitializeUnderlaySiteRequest, InitializeUnderlaySiteResponse,
    RegisterDeviceRequest, RegisterDeviceResponse, SecretStore, UnderlaySiteInitializationService,
};
use crate::engine::dry_run::{build_dry_run_plan, DryRunPlan};
use crate::intent::validation::validate_switch_pair_intent;
use crate::model::DeviceId;
use crate::planner::device_plan::{plan_switch_pair, DeviceDesiredState};
use crate::planner::domain_plan::plan_underlay_domain;
use crate::proto::adapter::RequestContext;
use crate::state::DeviceShadowState;
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
}

impl AriaUnderlayService {
    pub fn new(inventory: DeviceInventory) -> Self {
        Self {
            inventory,
            journal: Arc::new(InMemoryTxJournalStore::default()),
            endpoint_locks: EndpointLockTable::default(),
            lock_policy: LockAcquisitionPolicy::default(),
            secret_store: Arc::new(InMemorySecretStore::default()),
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
        }
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
            current_states.push(
                client
                    .get_current_state(&managed.info)
                    .await?,
            );
        }

        Ok(current_states)
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
    ) -> UnderlayResult<ApplyIntentResponse> {
        let mut device_results = Vec::with_capacity(desired_states.len());

        for desired in &desired_states {
            device_results.push(
                self.apply_single_endpoint_state(
                    &request_id,
                    &trace_id,
                    desired,
                    allow_degraded_atomicity,
                )
                    .await,
            );
        }

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

    async fn apply_single_endpoint_state(
        &self,
        request_id: &str,
        trace_id: &str,
        desired: &DeviceDesiredState,
        allow_degraded_atomicity: bool,
    ) -> DeviceApplyResult {
        let _endpoint_guard = match self
            .endpoint_locks
            .acquire_many_with_policy(std::slice::from_ref(&desired.device_id), &self.lock_policy)
            .await
        {
            Ok(guard) => guard,
            Err(err) => return device_error_result(&desired.device_id, false, None, None, err),
        };

        if let Err(err) = self.ensure_no_in_doubt_for_devices(std::slice::from_ref(&desired.device_id)) {
            return device_error_result(
                &desired.device_id,
                false,
                None,
                None,
                err,
            );
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
                journal_record = journal_record
                    .with_strategy(strategy)
                    .with_phase(TxPhase::Committed);
                if let Err(err) = self.journal.put(&journal_record) {
                    return device_error_result(
                        &desired.device_id,
                        true,
                        Some(tx_context.tx_id),
                        Some(strategy),
                        err,
                    );
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
                    warnings: degraded_strategy_warnings(strategy),
                }
            }
            Err(err) => {
                let (code, message) = journal_error_fields(&err);
                let phase = failed_apply_phase(&journal_record.phase);
                journal_record = journal_record
                    .with_phase(phase.clone())
                    .with_error(code.clone(), message.clone());
                let _ = self.journal.put(&journal_record);
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

        let _endpoint_guard = self.endpoint_locks.acquire_many(&record.devices).await?;
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
        let managed = self.inventory.get(&device_id)?;
        let mut client = AdapterClient::connect(managed.info.adapter_endpoint.clone()).await?;
        client.get_current_state(&managed.info).await
    }

    async fn recover_pending_transactions(&self) -> UnderlayResult<RecoveryReport> {
        let recoverable = self.journal.list_recoverable()?;
        let decisions = recoverable
            .iter()
            .map(classify_recovery)
            .collect::<Vec<_>>();
        let mut recovered = 0;

        for (record, decision) in recoverable.iter().zip(decisions.iter()) {
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

                    match self.recover_record(record, decision.action).await {
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
            let state = self.get_device_state(device_id.clone()).await?;
            if !state.warnings.is_empty() {
                self.inventory
                    .set_state(&device_id, crate::device::DeviceLifecycleState::Drifted)?;
                drifted_devices.push(device_id);
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

fn aggregate_apply_status(device_results: &[DeviceApplyResult]) -> ApplyStatus {
    if device_results
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
    } else if device_results.len() == 1 {
        device_results[0].status.clone()
    } else if device_results
        .iter()
        .any(|result| {
            matches!(
                result.status,
                ApplyStatus::Success | ApplyStatus::SuccessWithWarning | ApplyStatus::NoOpSuccess
            )
        })
    {
        ApplyStatus::SuccessWithWarning
    } else if device_results
        .iter()
        .all(|result| result.status == ApplyStatus::RolledBack)
    {
        ApplyStatus::RolledBack
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
}
