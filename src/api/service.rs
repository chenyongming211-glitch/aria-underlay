use async_trait::async_trait;

use crate::adapter_client::AdapterClient;
use crate::api::force_unlock::{ForceUnlockRequest, ForceUnlockResponse};
use crate::api::request::{ApplyIntentRequest, DriftAuditRequest, RefreshStateRequest};
use crate::api::response::{
    ApplyIntentResponse, ApplyStatus, DeviceApplyResult, DeviceOnboardingResponse,
    DriftAuditResponse, DryRunResponse, RefreshStateResponse,
};
use crate::device::{
    DeviceInventory, DeviceOnboardingService, DeviceRegistrationService,
    InitializeUnderlaySiteRequest, InitializeUnderlaySiteResponse, RegisterDeviceRequest,
    RegisterDeviceResponse,
};
use crate::api::underlay_service::UnderlayService;
use crate::engine::dry_run::{build_dry_run_plan, DryRunPlan};
use crate::intent::validation::validate_switch_pair_intent;
use crate::model::DeviceId;
use crate::planner::device_plan::{plan_switch_pair, DeviceDesiredState};
use crate::state::DeviceShadowState;
use crate::tx::recovery::RecoveryReport;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct AriaUnderlayService {
    inventory: DeviceInventory,
}

impl AriaUnderlayService {
    pub fn new(inventory: DeviceInventory) -> Self {
        Self { inventory }
    }

    async fn dry_run_plan(&self, request: &ApplyIntentRequest) -> UnderlayResult<DryRunPlan> {
        validate_switch_pair_intent(&request.intent)?;

        let desired_states = plan_switch_pair(&request.intent);
        let current_states = self.fetch_current_states(&desired_states).await?;
        build_dry_run_plan(&desired_states, &current_states)
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
        let trace_id = request
            .trace_id
            .clone()
            .unwrap_or_else(|| request.request_id.clone());
        let plan = self.dry_run_plan(&request).await?;
        let device_results = device_results_from_plan(&plan);

        if plan.is_noop() {
            return Ok(ApplyIntentResponse {
                request_id: request.request_id,
                trace_id,
                tx_id: None,
                status: ApplyStatus::NoOpSuccess,
                strategy: None,
                device_results,
                warnings: Vec::new(),
            });
        }

        Err(UnderlayError::UnsupportedTransactionStrategy)
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
        Err(UnderlayError::UnsupportedTransactionStrategy)
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
