use tonic::transport::Channel;

use crate::adapter_client::mapper::{
    adapter_result_to_outcome, capability_from_proto, desired_state_to_proto, device_ref_from_info,
    extract_adapter_errors, shadow_state_from_proto, strategy_to_proto, AdapterOutcome,
};
use crate::device::{DeviceCapabilityProfile, DeviceInfo};
use crate::planner::device_plan::DeviceDesiredState;
use crate::proto::adapter::underlay_adapter_client::UnderlayAdapterClient;
use crate::proto::adapter::{
    CommitRequest, GetCapabilitiesRequest, GetCurrentStateRequest, PrepareRequest, RequestContext,
    RollbackRequest, VerifyRequest,
};
use crate::state::DeviceShadowState;
use crate::tx::TransactionStrategy;
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
        let response = self
            .inner
            .get_current_state(GetCurrentStateRequest {
                context: Some(request_context(device)),
                device: Some(device_ref_from_info(device)),
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
        let response = self
            .inner
            .prepare(PrepareRequest {
                context: Some(request_context(device)),
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
        let response = self
            .inner
            .commit(CommitRequest {
                context: Some(request_context(device)),
                device: Some(device_ref_from_info(device)),
                strategy: strategy_to_proto(strategy) as i32,
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

    pub async fn rollback(&mut self, device: &DeviceInfo) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .rollback(RollbackRequest {
                context: Some(request_context(device)),
                device: Some(device_ref_from_info(device)),
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

    pub async fn verify(
        &mut self,
        device: &DeviceInfo,
        desired_state: &DeviceDesiredState,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .verify(VerifyRequest {
                context: Some(request_context(device)),
                device: Some(device_ref_from_info(device)),
                desired_state: Some(desired_state_to_proto(desired_state)),
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
