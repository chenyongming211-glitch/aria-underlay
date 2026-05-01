use async_trait::async_trait;
use std::sync::Arc;

use crate::adapter_client::AdapterClientPool;
use crate::api::admin_ops::AdminOps;
use crate::api::apply::device_results_from_plan;
use crate::api::apply_coordinator::ApplyCoordinator;
use crate::api::force_resolve::{
    ForceResolveTransactionRequest, ForceResolveTransactionResponse,
};
use crate::api::force_unlock::{ForceUnlockRequest, ForceUnlockResponse};
use crate::api::request::{
    ApplyDomainIntentRequest, ApplyIntentRequest, DriftAuditRequest, RefreshStateRequest,
};
use crate::api::response::{
    ApplyIntentResponse, DeviceOnboardingResponse, DriftAuditResponse, DryRunResponse,
    RefreshStateResponse,
};
use crate::api::transactions::{
    ListInDoubtTransactionsRequest, ListInDoubtTransactionsResponse,
};
use crate::api::underlay_service::UnderlayService;
use crate::api::recovery_coordinator::RecoveryCoordinator;
use crate::device::{
    DeviceInventory, DeviceLifecycleState, DeviceOnboardingService,
    DeviceRegistrationService, InMemorySecretStore, InitializeUnderlaySiteRequest,
    InitializeUnderlaySiteResponse, RegisterDeviceRequest, RegisterDeviceResponse, SecretStore,
    UnderlaySiteInitializationService,
};
use crate::intent::validation::validate_switch_pair_intent;
use crate::model::DeviceId;
use crate::planner::device_plan::plan_switch_pair;
use crate::planner::domain_plan::plan_underlay_domain;
use crate::state::drift::{detect_drift, DriftPolicy, DriftReport};
use crate::state::{
    DeviceShadowState, InMemoryShadowStateStore, ShadowStateStore,
};
use crate::telemetry::{EventSink, NoopEventSink, UnderlayEvent};
use crate::tx::recovery::RecoveryReport;
use crate::tx::{
    EndpointLockTable, InMemoryTxJournalStore, LockAcquisitionPolicy, TxJournalStore,
};
use crate::UnderlayResult;

#[derive(Debug, Clone)]
pub struct AriaUnderlayService {
    inventory: DeviceInventory,
    journal: Arc<dyn TxJournalStore>,
    endpoint_locks: EndpointLockTable,
    lock_policy: LockAcquisitionPolicy,
    secret_store: Arc<dyn SecretStore>,
    shadow_store: Arc<dyn ShadowStateStore>,
    observed_store: Arc<dyn ShadowStateStore>,
    event_sink: Arc<dyn EventSink>,
    adapter_pool: AdapterClientPool,
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
            observed_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
            adapter_pool: AdapterClientPool::default(),
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
            observed_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
            adapter_pool: AdapterClientPool::default(),
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
            observed_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
            adapter_pool: AdapterClientPool::default(),
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
            observed_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
            adapter_pool: AdapterClientPool::default(),
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
            observed_store: Arc::new(InMemoryShadowStateStore::default()),
            event_sink: Arc::new(NoopEventSink),
            adapter_pool: AdapterClientPool::default(),
        }
    }

    pub fn with_observed_state_store(mut self, observed_store: Arc<dyn ShadowStateStore>) -> Self {
        self.observed_store = observed_store;
        self
    }

    pub fn with_event_sink(mut self, event_sink: Arc<dyn EventSink>) -> Self {
        self.event_sink = event_sink;
        self
    }

    pub fn with_adapter_pool(mut self, adapter_pool: AdapterClientPool) -> Self {
        self.adapter_pool = adapter_pool;
        self
    }

    fn apply_coordinator(&self) -> ApplyCoordinator {
        ApplyCoordinator::new(
            self.inventory.clone(),
            self.journal.clone(),
            self.endpoint_locks.clone(),
            self.lock_policy.clone(),
            self.shadow_store.clone(),
            self.observed_store.clone(),
            self.event_sink.clone(),
            self.adapter_pool.clone(),
        )
    }

    fn recovery_coordinator(&self) -> RecoveryCoordinator {
        RecoveryCoordinator::new(
            self.inventory.clone(),
            self.journal.clone(),
            self.endpoint_locks.clone(),
            self.adapter_pool.clone(),
        )
    }

    fn admin_ops(&self) -> AdminOps {
        AdminOps::new(
            self.inventory.clone(),
            self.journal.clone(),
            self.endpoint_locks.clone(),
            self.event_sink.clone(),
            self.adapter_pool.clone(),
        )
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
        self.apply_coordinator()
            .apply_desired_states(
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
        let desired_states = plan_underlay_domain(&request.intent)?;
        let plan = self
            .apply_coordinator()
            .dry_run_desired_states(&desired_states)
            .await?;
        Ok(DryRunResponse {
            device_results: device_results_from_plan(&plan),
            noop: plan.is_noop(),
            change_sets: plan.change_sets,
        })
    }

    async fn fetch_device_state_from_adapter(
        &self,
        device_id: &DeviceId,
    ) -> UnderlayResult<DeviceShadowState> {
        let managed = self.inventory.get(device_id)?;
        let mut client = self.adapter_pool.client(&managed.info.adapter_endpoint)?;
        client.get_current_state(&managed.info).await
    }

    #[cfg(test)]
    fn ensure_drift_policy_allows_apply(
        &self,
        device_ids: &[DeviceId],
        policy: DriftPolicy,
    ) -> UnderlayResult<()> {
        self.apply_coordinator()
            .ensure_drift_policy_allows_apply(device_ids, policy)
    }
}

#[async_trait]
impl UnderlayService for AriaUnderlayService {
    async fn initialize_underlay_site(
        &self,
        request: InitializeUnderlaySiteRequest,
    ) -> UnderlayResult<InitializeUnderlaySiteResponse> {
        UnderlaySiteInitializationService::new_with_secret_store_and_adapter_pool(
            self.inventory.clone(),
            self.secret_store.clone(),
            self.adapter_pool.clone(),
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
        let lifecycle_state = DeviceOnboardingService::new_with_adapter_pool(
            self.inventory.clone(),
            self.adapter_pool.clone(),
        )
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
        self.apply_coordinator()
            .apply_desired_states(
                request_id,
                trace_id,
                desired_states,
                request.options.allow_degraded_atomicity,
                request.options.drift_policy,
            )
            .await
    }

    async fn dry_run(&self, request: ApplyIntentRequest) -> UnderlayResult<DryRunResponse> {
        validate_switch_pair_intent(&request.intent)?;
        let desired_states = plan_switch_pair(&request.intent);
        let plan = self
            .apply_coordinator()
            .dry_run_desired_states(&desired_states)
            .await?;
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
        self.observed_store.put(state.clone())?;
        Ok(state)
    }

    async fn recover_pending_transactions(&self) -> UnderlayResult<RecoveryReport> {
        self.recovery_coordinator()
            .recover_pending_transactions()
            .await
    }

    async fn list_in_doubt_transactions(
        &self,
        request: ListInDoubtTransactionsRequest,
    ) -> UnderlayResult<ListInDoubtTransactionsResponse> {
        self.admin_ops().list_in_doubt_transactions(request)
    }

    async fn run_drift_audit(
        &self,
        request: DriftAuditRequest,
    ) -> UnderlayResult<DriftAuditResponse> {
        let mut drifted_devices = Vec::new();

        for device_id in request.device_ids {
            let observed = self.fetch_device_state_from_adapter(&device_id).await?;
            self.observed_store.put(observed.clone())?;
            let expected = self.shadow_store.get(&device_id)?;
            let report = match expected.as_ref() {
                Some(expected) => detect_drift(expected, &observed),
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
            } else if expected.is_some() {
                let managed = self.inventory.get(&device_id)?;
                if managed.info.lifecycle_state == DeviceLifecycleState::Drifted {
                    self.inventory
                        .set_state(&device_id, DeviceLifecycleState::Ready)?;
                }
            }
        }

        Ok(DriftAuditResponse { drifted_devices })
    }

    async fn force_unlock(
        &self,
        request: ForceUnlockRequest,
    ) -> UnderlayResult<ForceUnlockResponse> {
        self.admin_ops().force_unlock(request).await
    }

    async fn force_resolve_transaction(
        &self,
        request: ForceResolveTransactionRequest,
    ) -> UnderlayResult<ForceResolveTransactionResponse> {
        self.admin_ops().force_resolve_transaction(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceInfo, HostKeyPolicy};
    use crate::model::{DeviceRole, Vendor};
    use crate::UnderlayError;

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
