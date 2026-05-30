use std::sync::Arc;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

use crate::adapter_client::mapper::{
    adapter_result_to_outcome, capability_from_proto, desired_state_to_proto, device_ref_from_info,
    extract_adapter_errors, recovery_action_to_proto, shadow_state_from_proto,
    state_scope_from_change_set, state_scope_from_desired, strategy_to_proto, AdapterOutcome,
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
use crate::tx::{RecoveryAction, TransactionStrategy, TxContext};
use crate::{UnderlayError, UnderlayResult};

pub const DEFAULT_CONFIRMED_COMMIT_TIMEOUT_SECS: u32 = 120;

#[derive(Debug, Clone)]
pub struct AdapterClient {
    inner: UnderlayAdapterClient<Channel>,
}

impl AdapterClient {
    pub fn from_channel(channel: Channel) -> Self {
        Self {
            inner: UnderlayAdapterClient::new(channel),
        }
    }

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
            acl_ids: Vec::new(),
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
        self.commit_with_context(
            device,
            &request_context(device),
            strategy,
            DEFAULT_CONFIRMED_COMMIT_TIMEOUT_SECS,
            None,
        )
        .await
    }

    pub async fn commit_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        strategy: TransactionStrategy,
        confirm_timeout_secs: u32,
        prepared_candidate_checksum: Option<&str>,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .commit(CommitRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                strategy: strategy_to_proto(strategy) as i32,
                confirm_timeout_secs,
                prepared_candidate_checksum: prepared_candidate_checksum
                    .unwrap_or_default()
                    .to_string(),
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
        strategy: Option<TransactionStrategy>,
        action: RecoveryAction,
    ) -> UnderlayResult<AdapterOutcome> {
        self.recover_with_context(device, &tx_request_context(device, tx), strategy, action)
            .await
    }

    pub async fn recover_with_context(
        &mut self,
        device: &DeviceInfo,
        context: &RequestContext,
        strategy: Option<TransactionStrategy>,
        action: RecoveryAction,
    ) -> UnderlayResult<AdapterOutcome> {
        let response = self
            .inner
            .recover(RecoverRequest {
                context: Some(context.clone()),
                device: Some(device_ref_from_info(device)),
                strategy: strategy
                    .map(strategy_to_proto)
                    .unwrap_or(crate::proto::adapter::TransactionStrategy::Unspecified)
                    as i32,
                action: recovery_action_to_proto(action) as i32,
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

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub client_cert_pem: String,
    pub client_key_pem: String,
    pub ca_cert_pem: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AdapterClientPool {
    channels: Arc<DashMap<String, Channel>>,
    tls_config: Option<TlsConfig>,
}

impl AdapterClientPool {
    pub fn with_tls(tls_config: TlsConfig) -> Self {
        Self {
            channels: Arc::new(DashMap::new()),
            tls_config: Some(tls_config),
        }
    }

    pub fn client(&self, endpoint: &str) -> UnderlayResult<AdapterClient> {
        match self.channels.entry(endpoint.to_string()) {
            Entry::Occupied(entry) => Ok(AdapterClient::from_channel(entry.get().clone())),
            Entry::Vacant(entry) => {
                let mut builder = Channel::from_shared(endpoint.to_string())
                    .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?;
                if let Some(tls) = &self.tls_config {
                    let mut client_tls = ClientTlsConfig::new()
                        .with_enabled_roots();
                    if let Some(ca_pem) = &tls.ca_cert_pem {
                        client_tls = client_tls
                            .ca_certificate(Certificate::from_pem(ca_pem));
                    }
                    client_tls = client_tls.identity(Identity::from_pem(
                        &tls.client_cert_pem,
                        &tls.client_key_pem,
                    ));
                    builder = builder
                        .tls_config(client_tls)
                        .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?;
                }
                let channel = builder.connect_lazy();
                entry.insert(channel.clone());
                Ok(AdapterClient::from_channel(channel))
            }
        }
    }

    pub fn invalidate(&self, endpoint: &str) {
        self.channels.remove(endpoint);
    }

    pub fn cached_endpoint_count(&self) -> usize {
        self.channels.len()
    }

    pub fn contains_endpoint(&self, endpoint: &str) -> bool {
        self.channels.contains_key(endpoint)
    }

    pub fn has_tls(&self) -> bool {
        self.tls_config.is_some()
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
