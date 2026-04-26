use tonic::transport::Channel;

use crate::adapter_client::mapper::{
    adapter_result_to_outcome, capability_from_proto, desired_state_to_proto, device_ref_from_info,
    extract_adapter_errors, shadow_state_from_proto, state_scope_from_change_set,
    state_scope_from_desired, strategy_to_proto, AdapterOutcome,
};
use crate::device::{DeviceCapabilityProfile, DeviceInfo};
use crate::engine::diff::ChangeSet;
use crate::planner::device_plan::DeviceDesiredState;
use crate::proto::adapter::underlay_adapter_client::UnderlayAdapterClient;
use crate::proto::adapter::{
    CommitRequest, FinalConfirmRequest, ForceUnlockRequest, GetCapabilitiesRequest,
    GetCurrentStateRequest, PrepareRequest, RecoverRequest, RequestContext, RollbackRequest,
    StateScope, VerifyRequest,
};
use crate::state::DeviceShadowState;
use crate::tx::{TransactionStrategy, TxContext};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct AdapterClient {
    inner: UnderlayAdapterClient<Channel>,
}

impl AdapterClient {
    pub async fn connect(endpoint: String) -> UnderlayResult<Self> {
        let inner = UnderlayAdapterClient::connect(endpoint)
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?;
        Ok(Self { inner })
    }

    pub async fn get_capabilities(
        &mut self,
        device: &DeviceInfo,
    ) -> UnderlayResult<DeviceCapabilityProfile> {
        let request = GetCapabilitiesRequest {
            context: Some(RequestContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                tx_id: String::new(),
                trace_id: uuid::Uuid::new_v4().to_string(),
                tenant_id: device.tenant_id.clone(),
                site_id: device.site_id.clone(),
            }),
            device: Some(device_ref_from_info(device)),
        };

        let response = self
            .inner
            .get_capabilities(request)
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        if let Some(error) = extract_adapter_errors(response.errors) {
            return Err(error);
        }

        let capability = response
            .capability
            .ok_or_else(|| UnderlayError::AdapterOperation {
                code: "MISSING_CAPABILITY".into(),
                message: "adapter returned no capability".into(),
                retryable: false,
                errors: Vec::new(),
            })?;

        Ok(capability_from_proto(capability, response.warnings))
    }

    pub async fn get_current_state(
        &mut self,
        device: &DeviceInfo,
    ) -> UnderlayResult<DeviceShadowState> {
        self.get_current_state_with_scope(device, StateScope {
            full: true,
            vlan_ids: Vec::new(),
            interface_names: Vec::new(),
        })
        .await
    }

    pub async fn get_current_state_for_desired(
        &mut self,
        device: &DeviceInfo,
        desired_state: &DeviceDesiredState,
    ) -> UnderlayResult<DeviceShadowState> {
        self.get_current_state_with_scope(device, state_scope_from_desired(desired_state))
            .await
    }

    async fn get_current_state_with_scope(
        &mut self,
        device: &DeviceInfo,
        scope: StateScope,
    ) -> UnderlayResult<DeviceShadowState> {
        let response = self
            .inner
            .get_current_state(GetCurrentStateRequest {
                context: Some(request_context(device)),
                device: Some(device_ref_from_info(device)),
                scope: Some(scope),
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        if let Some(error) = extract_adapter_errors(response.errors) {
            return Err(error);
        }

        let state = response.state.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_STATE".into(),
            message: "adapter returned no current state".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        shadow_state_from_proto(state, response.warnings)
    }

    pub async fn prepare(
        &mut self,
        device: &DeviceInfo,
        desired_state: &DeviceDesiredState,
    ) -> UnderlayResult<AdapterOutcome> {
        self.prepare_with_context(device, &request_context(device), desired_state)
            .await
    }

    pub async fn prepare_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        desired_state: &DeviceDesiredState,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .prepare(PrepareRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                desired_state: Some(desired_state_to_proto(desired_state)),
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no prepare result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }

    pub async fn commit(
        &mut self,
        device: &DeviceInfo,
        strategy: TransactionStrategy,
    ) -> UnderlayResult<AdapterOutcome> {
        self.commit_with_context(device, &request_context(device), strategy)
            .await
    }

    pub async fn commit_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        strategy: TransactionStrategy,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .commit(CommitRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                strategy: strategy_to_proto(strategy) as i32,
                confirm_timeout_secs: 120,
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no commit result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }

    pub async fn final_confirm_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .final_confirm(FinalConfirmRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no final confirm result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }

    pub async fn rollback(&mut self, device: &DeviceInfo) -> UnderlayResult<AdapterOutcome> {
        self.rollback_with_context(device, &request_context(device), None)
            .await
    }

    pub async fn rollback_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        strategy: Option<TransactionStrategy>,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .rollback(RollbackRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                strategy: strategy
                    .map(strategy_to_proto)
                    .unwrap_or(crate::proto::adapter::TransactionStrategy::Unspecified)
                    as i32,
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no rollback result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }

    pub async fn recover(
        &mut self,
        device: &DeviceInfo,
        tx: &TxContext,
    ) -> UnderlayResult<AdapterOutcome> {
        self.recover_with_context(device, &tx_request_context(device, tx))
            .await
    }

    pub async fn recover_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .recover(RecoverRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no recover result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }

    pub async fn verify(
        &mut self,
        device: &DeviceInfo,
        desired_state: &DeviceDesiredState,
    ) -> UnderlayResult<AdapterOutcome> {
        self.verify_with_context(device, &request_context(device), desired_state)
            .await
    }

    pub async fn verify_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        desired_state: &DeviceDesiredState,
    ) -> UnderlayResult<AdapterOutcome> {
        self.verify_with_context_and_scope(
            device,
            context,
            desired_state,
            state_scope_from_desired(desired_state),
        )
        .await
    }

    pub async fn verify_with_context_for_change_set(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        desired_state: &DeviceDesiredState,
        change_set: &ChangeSet,
    ) -> UnderlayResult<AdapterOutcome> {
        self.verify_with_context_and_scope(
            device,
            context,
            desired_state,
            state_scope_from_change_set(change_set),
        )
        .await
    }

    async fn verify_with_context_and_scope(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        desired_state: &DeviceDesiredState,
        scope: StateScope,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .verify(VerifyRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                desired_state: Some(desired_state_to_proto(desired_state)),
                scope: Some(scope),
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no verify result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }

    pub async fn force_unlock(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        lock_owner: String,
        reason: String,
        break_glass_enabled: bool,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .force_unlock(ForceUnlockRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                lock_owner,
                reason,
                break_glass_enabled,
            })
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        let result = response.result.ok_or_else(|| UnderlayError::AdapterOperation {
            code: "MISSING_ADAPTER_RESULT".into(),
            message: "adapter returned no force unlock result".into(),
            retryable: false,
            errors: Vec::new(),
        })?;

        adapter_result_to_outcome(result)
    }
}

pub fn tx_request_context(device: &DeviceInfo, tx: &TxContext) -> RequestContext {
    RequestContext {
        request_id: tx.request_id.clone(),
        tx_id: tx.tx_id.clone(),
        trace_id: tx.trace_id.clone(),
        tenant_id: device.tenant_id.clone(),
        site_id: device.site_id.clone(),
    }
}

fn request_context(device: &DeviceInfo) -> RequestContext {
    RequestContext {
        request_id: uuid::Uuid::new_v4().to_string(),
        tx_id: String::new(),
        trace_id: uuid::Uuid::new_v4().to_string(),
        tenant_id: device.tenant_id.clone(),
        site_id: device.site_id.clone(),
    }
}
