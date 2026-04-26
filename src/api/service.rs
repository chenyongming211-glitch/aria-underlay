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
    DeviceInventory, DeviceOnboardingService, DeviceRegistrationService,
    InitializeUnderlaySiteRequest, InitializeUnderlaySiteResponse, RegisterDeviceRequest,
    RegisterDeviceResponse,
};
use crate::engine::dry_run::{build_dry_run_plan, DryRunPlan};
use crate::intent::validation::validate_switch_pair_intent;
use crate::model::DeviceId;
use crate::planner::device_plan::{plan_switch_pair, DeviceDesiredState};
use crate::planner::domain_plan::plan_underlay_domain;
use crate::state::DeviceShadowState;
use crate::tx::recovery::RecoveryReport;
use crate::tx::{
    EndpointLockTable, InMemoryTxJournalStore, TransactionStrategy, TxContext, TxJournalRecord,
    TxJournalStore, TxPhase,
};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct AriaUnderlayService {
    inventory: DeviceInventory,
    journal: Arc<dyn TxJournalStore>,
    endpoint_locks: EndpointLockTable,
}

impl AriaUnderlayService {
    pub fn new(inventory: DeviceInventory) -> Self {
        Self {
            inventory,
            journal: Arc::new(InMemoryTxJournalStore::default()),
            endpoint_locks: EndpointLockTable::default(),
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
        let _endpoint_guard = self
            .endpoint_locks
            .acquire_many(&desired_device_ids(&desired_states))
            .await?;
        let plan = self.dry_run_desired_states(&desired_states).await?;
        let device_results = device_results_from_plan(&plan);

        if plan.is_noop() {
            return Ok(ApplyIntentResponse {
                request_id,
                trace_id,
                tx_id: None,
                status: ApplyStatus::NoOpSuccess,
                strategy: None,
                device_results,
                warnings: Vec::new(),
            });
        }

        let tx_context = TxContext::new(request_id.clone(), trace_id.clone());
        let mut journal_record = TxJournalRecord::started(
            &tx_context,
            changed_device_ids(&plan),
        );
        self.journal.put(&journal_record)?;
        journal_record = journal_record.with_phase(TxPhase::Preparing);
        self.journal.put(&journal_record)?;

        let strategy = match self
            .apply_changed_endpoint_states(
                &desired_states,
                &plan,
                &tx_context,
                &mut journal_record,
            )
            .await
        {
            Ok(strategy) => {
                journal_record = journal_record
                    .with_strategy(strategy)
                    .with_phase(TxPhase::Committed);
                self.journal.put(&journal_record)?;
                strategy
            }
            Err(err) => {
                let (code, message) = journal_error_fields(&err);
                journal_record = journal_record
                    .with_phase(TxPhase::Failed)
                    .with_error(code, message);
                self.journal.put(&journal_record)?;
                return Err(err);
            }
        };
        Ok(ApplyIntentResponse {
            request_id,
            trace_id,
            tx_id: Some(tx_context.tx_id),
            status: ApplyStatus::Success,
            strategy: Some(strategy),
            device_results,
            warnings: Vec::new(),
        })
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
            current_states.push(client.get_current_state(&managed.info).await?);
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

    async fn apply_changed_endpoint_states(
        &self,
        desired_states: &[DeviceDesiredState],
        plan: &DryRunPlan,
        tx_context: &TxContext,
        journal_record: &mut TxJournalRecord,
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
            selected_strategy.get_or_insert(strategy);
            *journal_record = journal_record.clone().with_strategy(strategy);
            self.journal.put(journal_record)?;

            let prepare = client
                .prepare_with_context(&managed.info, &rpc_context, desired)
                .await?;
            if prepare.status != AdapterOperationStatus::Prepared {
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
                Ok(commit) if commit.status == AdapterOperationStatus::Committed => {}
                Ok(commit) => {
                    *journal_record = journal_record.clone().with_phase(TxPhase::RollingBack);
                    self.journal.put(journal_record)?;
                    let _ = client
                        .rollback_with_context(&managed.info, &rpc_context)
                        .await;
                    return Err(UnderlayError::AdapterOperation {
                        code: "UNEXPECTED_COMMIT_STATUS".into(),
                        message: format!("adapter returned commit status {:?}", commit.status),
                        retryable: false,
                        errors: Vec::new(),
                    });
                }
                Err(err) => {
                    *journal_record = journal_record.clone().with_phase(TxPhase::RollingBack);
                    self.journal.put(journal_record)?;
                    let _ = client
                        .rollback_with_context(&managed.info, &rpc_context)
                        .await;
                    return Err(err);
                }
            }

            *journal_record = journal_record.clone().with_phase(TxPhase::Verifying);
            self.journal.put(journal_record)?;
            match client
                .verify_with_context(&managed.info, &rpc_context, desired)
                .await
            {
                Ok(verify)
                    if matches!(
                        verify.status,
                        AdapterOperationStatus::NoChange | AdapterOperationStatus::Committed
                    ) => {}
                Ok(verify) => {
                    *journal_record = journal_record.clone().with_phase(TxPhase::RollingBack);
                    self.journal.put(journal_record)?;
                    let _ = client
                        .rollback_with_context(&managed.info, &rpc_context)
                        .await;
                    return Err(UnderlayError::AdapterOperation {
                        code: "UNEXPECTED_VERIFY_STATUS".into(),
                        message: format!("adapter returned verify status {:?}", verify.status),
                        retryable: false,
                        errors: Vec::new(),
                    });
                }
                Err(err) => {
                    *journal_record = journal_record.clone().with_phase(TxPhase::RollingBack);
                    self.journal.put(journal_record)?;
                    let _ = client
                        .rollback_with_context(&managed.info, &rpc_context)
                        .await;
                    return Err(err);
                }
            }
        }

        selected_strategy.ok_or(UnderlayError::UnsupportedTransactionStrategy)
    }
}

#[async_trait]
impl UnderlayService for AriaUnderlayService {
    async fn initialize_underlay_site(
        &self,
        _request: InitializeUnderlaySiteRequest,
    ) -> UnderlayResult<InitializeUnderlaySiteResponse> {
        Err(UnderlayError::UnsupportedTransactionStrategy)
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
        let _endpoint_guard = self
            .endpoint_locks
            .acquire_many(&desired_device_ids(&desired_states))
            .await?;
        let plan = self.dry_run_desired_states(&desired_states).await?;
        let device_results = device_results_from_plan(&plan);

        if plan.is_noop() {
            return Ok(ApplyIntentResponse {
                request_id,
                trace_id,
                tx_id: None,
                status: ApplyStatus::NoOpSuccess,
                strategy: None,
                device_results,
                warnings: Vec::new(),
            });
        }

        let tx_context = TxContext::new(request_id.clone(), trace_id.clone());
        let mut journal_record = TxJournalRecord::started(
            &tx_context,
            changed_device_ids(&plan),
        );
        self.journal.put(&journal_record)?;
        journal_record = journal_record.with_phase(TxPhase::Preparing);
        self.journal.put(&journal_record)?;

        let strategy = match self
            .apply_changed_endpoint_states(
                &desired_states,
                &plan,
                &tx_context,
                &mut journal_record,
            )
            .await
        {
            Ok(strategy) => {
                journal_record = journal_record
                    .with_strategy(strategy)
                    .with_phase(TxPhase::Committed);
                self.journal.put(&journal_record)?;
                strategy
            }
            Err(err) => {
                let (code, message) = journal_error_fields(&err);
                journal_record = journal_record
                    .with_phase(TxPhase::Failed)
                    .with_error(code, message);
                self.journal.put(&journal_record)?;
                return Err(err);
            }
        };
        Ok(ApplyIntentResponse {
            request_id,
            trace_id,
            tx_id: Some(tx_context.tx_id),
            status: ApplyStatus::Success,
            strategy: Some(strategy),
            device_results,
            warnings: Vec::new(),
        })
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
        let in_doubt = recoverable
            .iter()
            .filter(|record| record.phase == TxPhase::InDoubt)
            .count();
        let tx_ids = recoverable
            .iter()
            .map(|record| record.tx_id.clone())
            .collect::<Vec<_>>();

        Ok(RecoveryReport {
            recovered: 0,
            in_doubt,
            pending: recoverable.len(),
            tx_ids,
        })
    }

    async fn run_drift_audit(
        &self,
        _request: DriftAuditRequest,
    ) -> UnderlayResult<DriftAuditResponse> {
        Err(UnderlayError::UnsupportedTransactionStrategy)
    }

    async fn force_unlock(
        &self,
        _request: ForceUnlockRequest,
    ) -> UnderlayResult<ForceUnlockResponse> {
        Err(UnderlayError::UnsupportedTransactionStrategy)
    }
}

fn device_results_from_plan(plan: &DryRunPlan) -> Vec<DeviceApplyResult> {
    plan.change_sets
        .iter()
        .map(|change_set| DeviceApplyResult {
            device_id: change_set.device_id.clone(),
            changed: !change_set.is_empty(),
            warnings: Vec::new(),
        })
        .collect()
}

fn changed_device_ids(plan: &DryRunPlan) -> Vec<DeviceId> {
    plan.change_sets
        .iter()
        .filter(|change_set| !change_set.is_empty())
        .map(|change_set| change_set.device_id.clone())
        .collect()
}

fn desired_device_ids(desired_states: &[DeviceDesiredState]) -> Vec<DeviceId> {
    desired_states
        .iter()
        .map(|desired| desired.device_id.clone())
        .collect()
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
