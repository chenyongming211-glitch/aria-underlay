use std::sync::Arc;

use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::adapter_client::{tx_request_context, AdapterClient, AdapterClientPool};
use crate::api::apply::{
    aggregate_apply_status, apply_status_for_failed_phase, commit_status_matches_strategy,
    degraded_strategy_warnings, device_error_result, failed_apply_phase, journal_error_fields,
};
use crate::api::drift_ops::drift_policy_error;
use crate::api::response::{ApplyIntentResponse, ApplyStatus, DeviceApplyResult};
use crate::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState};
use crate::engine::dry_run::{build_dry_run_plan, DryRunPlan};
use crate::model::DeviceId;
use crate::planner::device_plan::DeviceDesiredState;
use crate::proto::adapter::RequestContext;
use crate::state::drift::DriftPolicy;
use crate::state::{DeviceShadowState, ShadowStateStore};
use crate::telemetry::{EventSink, UnderlayEvent};
use crate::tx::recovery::in_doubt_records_for_devices;
use crate::tx::{
    EndpointLockTable, LockAcquisitionPolicy, TransactionStrategy, TxContext, TxJournalRecord,
    TxJournalStore, TxPhase,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Clone)]
pub(crate) struct ApplyCoordinator {
    inventory: DeviceInventory,
    journal: Arc<dyn TxJournalStore>,
    endpoint_locks: EndpointLockTable,
    lock_policy: LockAcquisitionPolicy,
    shadow_store: Arc<dyn ShadowStateStore>,
    observed_store: Arc<dyn ShadowStateStore>,
    event_sink: Arc<dyn EventSink>,
    adapter_pool: AdapterClientPool,
    confirmed_commit_timeout_secs: u32,
}

impl ApplyCoordinator {
    pub(crate) fn new(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
        endpoint_locks: EndpointLockTable,
        lock_policy: LockAcquisitionPolicy,
        shadow_store: Arc<dyn ShadowStateStore>,
        observed_store: Arc<dyn ShadowStateStore>,
        event_sink: Arc<dyn EventSink>,
        adapter_pool: AdapterClientPool,
        confirmed_commit_timeout_secs: u32,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks,
            lock_policy,
            shadow_store,
            observed_store,
            event_sink,
            adapter_pool,
            confirmed_commit_timeout_secs: confirmed_commit_timeout_secs.max(1),
        }
    }

    pub(crate) async fn dry_run_desired_states(
        &self,
        desired_states: &[DeviceDesiredState],
    ) -> UnderlayResult<DryRunPlan> {
        let current_states = self.fetch_current_states(desired_states).await?;
        build_dry_run_plan(desired_states, &current_states)
    }

    pub(crate) async fn apply_desired_states(
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

    pub(crate) fn ensure_drift_policy_allows_apply(
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
        drift_policy_error(policy, &device_list)
    }

    async fn fetch_current_states(
        &self,
        desired_states: &[DeviceDesiredState],
    ) -> UnderlayResult<Vec<DeviceShadowState>> {
        let mut current_states = Vec::with_capacity(desired_states.len());

        for desired in desired_states {
            let managed = self.inventory.get(&desired.device_id)?;
            let mut client = self.adapter_pool.client(&managed.info.adapter_endpoint)?;
            // Preflight diff needs an authoritative current view. If we scope this
            // only to desired objects, absent desired resources cannot be detected
            // as deletes. Post-commit verify is scoped by ChangeSet below.
            let current = client.get_current_state(&managed.info).await?;
            self.observed_store.put(current.clone())?;
            current_states.push(current);
        }

        Ok(current_states)
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
            TxJournalRecord::started(&tx_context, vec![desired.device_id.clone()])
                .with_desired_states(vec![desired.clone()])
                .with_change_sets(plan.change_sets.clone());
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
            Ok(strategy) => self.finish_successful_apply(
                desired,
                tx_context,
                journal_record,
                strategy,
            ),
            Err(err) => self.finish_failed_apply(desired, tx_context, journal_record, err),
        }
    }

    fn finish_successful_apply(
        &self,
        desired: &DeviceDesiredState,
        tx_context: TxContext,
        mut journal_record: TxJournalRecord,
        strategy: TransactionStrategy,
    ) -> DeviceApplyResult {
        let mut warnings = degraded_strategy_warnings(strategy);
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
                    "adapter committed, \
                     but terminal journal write failed: {message}"
                )),
                warnings,
            };
        }
        let shadow_state = DeviceShadowState::from_desired(desired, 0);
        if let Err(err) = self.shadow_store.put(shadow_state) {
            let (code, message) = journal_error_fields(&err);
            let error_message = format!("shadow state stale after successful apply: {message}");
            journal_record = journal_record
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

    fn finish_failed_apply(
        &self,
        desired: &DeviceDesiredState,
        tx_context: TxContext,
        mut journal_record: TxJournalRecord,
        err: UnderlayError,
    ) -> DeviceApplyResult {
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
            let mut client = self.adapter_pool.client(&managed.info.adapter_endpoint)?;
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

            self.prepare_endpoint(
                &mut client,
                &managed.info,
                &rpc_context,
                desired,
                journal_record,
            )
            .await?;
            self.commit_endpoint(
                &mut client,
                &managed.info,
                &rpc_context,
                strategy,
                journal_record,
            )
            .await?;
            self.verify_endpoint(
                &mut client,
                &managed.info,
                &rpc_context,
                desired,
                change_set,
                journal_record,
            )
            .await?;

            if strategy == TransactionStrategy::ConfirmedCommit {
                self.final_confirm_endpoint(
                    &mut client,
                    &managed.info,
                    &rpc_context,
                    journal_record,
                )
                .await?;
            }
        }

        selected_strategy.ok_or(UnderlayError::UnsupportedTransactionStrategy)
    }

    async fn prepare_endpoint(
        &self,
        client: &mut AdapterClient,
        device: &DeviceInfo,
        context: &RequestContext,
        desired: &DeviceDesiredState,
        journal_record: &mut TxJournalRecord,
    ) -> UnderlayResult<()> {
        let prepare = match client.prepare_with_context(device, context, desired).await {
            Ok(prepare) => prepare,
            Err(err) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                return Err(err);
            }
        };
        if prepare.status != AdapterOperationStatus::Prepared {
            self.rollback_after_endpoint_failure(client, device, context, journal_record)
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
        Ok(())
    }

    async fn commit_endpoint(
        &self,
        client: &mut AdapterClient,
        device: &DeviceInfo,
        context: &RequestContext,
        strategy: TransactionStrategy,
        journal_record: &mut TxJournalRecord,
    ) -> UnderlayResult<()> {
        *journal_record = journal_record.clone().with_phase(TxPhase::Committing);
        self.journal.put(journal_record)?;
        match client
            .commit_with_context(
                device,
                context,
                strategy,
                self.confirmed_commit_timeout_secs,
            )
            .await
        {
            Ok(commit) if commit_status_matches_strategy(commit.status, strategy) => Ok(()),
            Ok(commit) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                Err(UnderlayError::AdapterOperation {
                    code: "UNEXPECTED_COMMIT_STATUS".into(),
                    message: format!("adapter returned commit status {:?}", commit.status),
                    retryable: false,
                    errors: Vec::new(),
                })
            }
            Err(err) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                Err(err)
            }
        }
    }

    async fn verify_endpoint(
        &self,
        client: &mut AdapterClient,
        device: &DeviceInfo,
        context: &RequestContext,
        desired: &DeviceDesiredState,
        change_set: &crate::engine::diff::ChangeSet,
        journal_record: &mut TxJournalRecord,
    ) -> UnderlayResult<()> {
        *journal_record = journal_record.clone().with_phase(TxPhase::Verifying);
        self.journal.put(journal_record)?;
        match client
            .verify_with_context_for_change_set(device, context, desired, change_set)
            .await
        {
            Ok(verify)
                if matches!(
                    verify.status,
                    AdapterOperationStatus::NoChange | AdapterOperationStatus::Committed
                ) =>
            {
                Ok(())
            }
            Ok(verify) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                Err(UnderlayError::AdapterOperation {
                    code: "UNEXPECTED_VERIFY_STATUS".into(),
                    message: format!("adapter returned verify status {:?}", verify.status),
                    retryable: false,
                    errors: Vec::new(),
                })
            }
            Err(err) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                Err(err)
            }
        }
    }

    async fn final_confirm_endpoint(
        &self,
        client: &mut AdapterClient,
        device: &DeviceInfo,
        context: &RequestContext,
        journal_record: &mut TxJournalRecord,
    ) -> UnderlayResult<()> {
        *journal_record = journal_record.clone().with_phase(TxPhase::FinalConfirming);
        self.journal.put(journal_record)?;
        match client.final_confirm_with_context(device, context).await {
            Ok(confirm) if confirm.status == AdapterOperationStatus::Committed => Ok(()),
            Ok(confirm) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                Err(UnderlayError::AdapterOperation {
                    code: "UNEXPECTED_FINAL_CONFIRM_STATUS".into(),
                    message: format!(
                        "adapter returned final confirm status {:?}",
                        confirm.status
                    ),
                    retryable: false,
                    errors: Vec::new(),
                })
            }
            Err(err) => {
                self.rollback_after_endpoint_failure(client, device, context, journal_record)
                    .await?;
                Err(err)
            }
        }
    }

    async fn rollback_after_endpoint_failure(
        &self,
        client: &mut AdapterClient,
        device: &DeviceInfo,
        context: &RequestContext,
        journal_record: &mut TxJournalRecord,
    ) -> UnderlayResult<()> {
        let rollback_result = client
            .rollback_with_context(device, context, journal_record.strategy)
            .await;

        *journal_record = journal_record.clone().with_phase(TxPhase::RollingBack);
        self.journal.put(journal_record)?;

        match rollback_result {
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
                let error = UnderlayError::AdapterOperation {
                    code: "UNEXPECTED_ROLLBACK_STATUS".into(),
                    message: format!("adapter returned rollback status {:?}", outcome.status),
                    retryable: true,
                    errors: Vec::new(),
                };
                *journal_record = journal_record
                    .clone()
                    .with_phase(TxPhase::InDoubt)
                    .with_error(
                        "UNEXPECTED_ROLLBACK_STATUS",
                        format!("adapter returned rollback status {:?}", outcome.status),
                    );
                self.journal.put(journal_record)?;
                return Err(error);
            }
            Err(err) => {
                let (code, message) = journal_error_fields(&err);
                *journal_record = journal_record
                    .clone()
                    .with_phase(TxPhase::InDoubt)
                    .with_error(code, message);
                self.journal.put(journal_record)?;
                return Err(err);
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
        let code = if blocking.iter().all(|record| record.phase == TxPhase::InDoubt) {
            "TX_IN_DOUBT"
        } else {
            "TX_REQUIRES_RECOVERY"
        };
        Err(UnderlayError::AdapterOperation {
            code: code.into(),
            message: format!("endpoint has unresolved recoverable transaction(s): {tx_ids}"),
            retryable: false,
            errors: Vec::new(),
        })
    }
}
