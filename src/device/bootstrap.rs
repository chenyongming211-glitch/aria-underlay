use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::device::secret::{NetconfCredentialInput, SecretStore};
use crate::device::{
    DeviceInventory, DeviceLifecycleState, DeviceOnboardingService, DeviceRegistrationService,
    HostKeyPolicy, RegisterDeviceRequest,
};
use crate::model::{DeviceId, DeviceRole, Vendor};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeUnderlaySiteRequest {
    pub request_id: String,
    pub tenant_id: String,
    pub site_id: String,
    pub adapter_endpoint: String,
    pub switches: Vec<SwitchBootstrapRequest>,
    pub allow_degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchBootstrapRequest {
    pub device_id: DeviceId,
    pub role: DeviceRole,
    pub management_ip: String,
    pub management_port: u16,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
    pub host_key_policy: HostKeyPolicy,
    pub credential: NetconfCredentialInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteInitializationStatus {
    Ready,
    ReadyWithDegradedDevice,
    Failed,
    PartiallyRegistered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInitializationResult {
    pub device_id: DeviceId,
    pub role: DeviceRole,
    pub secret_ref: Option<String>,
    pub lifecycle_state: Option<DeviceLifecycleState>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeUnderlaySiteResponse {
    pub request_id: String,
    pub tenant_id: String,
    pub site_id: String,
    pub status: SiteInitializationStatus,
    pub devices: Vec<DeviceInitializationResult>,
}

#[derive(Debug, Clone)]
pub struct UnderlaySiteInitializationService {
    inventory: DeviceInventory,
    registration: DeviceRegistrationService,
    onboarding: DeviceOnboardingService,
    secret_store: Arc<dyn SecretStore>,
}

impl UnderlaySiteInitializationService {
    pub fn new<S>(inventory: DeviceInventory, secret_store: S) -> Self
    where
        S: SecretStore + 'static,
    {
        Self::new_with_secret_store(inventory, Arc::new(secret_store))
    }

    pub fn new_with_secret_store(
        inventory: DeviceInventory,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            registration: DeviceRegistrationService::new(inventory.clone()),
            onboarding: DeviceOnboardingService::new(inventory.clone()),
            inventory,
            secret_store,
        }
    }

    pub async fn initialize_site(
        &self,
        request: InitializeUnderlaySiteRequest,
    ) -> UnderlayResult<InitializeUnderlaySiteResponse> {
        validate_switch_pair(&request)?;

        let mut devices = Vec::with_capacity(request.switches.len());

        for switch in request.switches.iter() {
            let result = self.initialize_switch(&request, switch).await;
            devices.push(result);
        }

        let status = summarize_status(&devices, request.allow_degraded);

        Ok(InitializeUnderlaySiteResponse {
            request_id: request.request_id,
            tenant_id: request.tenant_id,
            site_id: request.site_id,
            status,
            devices,
        })
    }

    async fn initialize_switch(
        &self,
        request: &InitializeUnderlaySiteRequest,
        switch: &SwitchBootstrapRequest,
    ) -> DeviceInitializationResult {
        let secret = match self.secret_store.create_for_device(
            &request.tenant_id,
            &request.site_id,
            &switch.device_id,
            switch.credential.clone(),
        ) {
            Ok(secret_ref) => secret_ref,
            Err(error) => {
                return DeviceInitializationResult {
                    device_id: switch.device_id.clone(),
                    role: switch.role,
                    secret_ref: None,
                    lifecycle_state: None,
                    error: Some(error.to_string()),
                };
            }
        };
        let secret_ref = secret.secret_ref.clone();

        let register_result = self.registration.register(RegisterDeviceRequest {
            tenant_id: request.tenant_id.clone(),
            site_id: request.site_id.clone(),
            device_id: switch.device_id.clone(),
            management_ip: switch.management_ip.clone(),
            management_port: switch.management_port,
            vendor_hint: switch.vendor_hint,
            model_hint: switch.model_hint.clone(),
            role: switch.role,
            secret_ref: secret_ref.clone(),
            host_key_policy: switch.host_key_policy.clone(),
            adapter_endpoint: request.adapter_endpoint.clone(),
        });

        if let Err(error) = register_result {
            if secret.cleanup_on_registration_failure {
                return self.registration_failure_result_with_secret_cleanup(
                    switch,
                    secret_ref,
                    error,
                );
            }

            return DeviceInitializationResult {
                device_id: switch.device_id.clone(),
                role: switch.role,
                secret_ref: Some(secret_ref),
                lifecycle_state: None,
                error: Some(error.to_string()),
            };
        }

        let onboarding_result = self.onboarding.onboard_device(switch.device_id.clone()).await;
        let lifecycle_state = self
            .inventory
            .get(&switch.device_id)
            .ok()
            .map(|managed| managed.info.lifecycle_state);

        DeviceInitializationResult {
            device_id: switch.device_id.clone(),
            role: switch.role,
            secret_ref: Some(secret_ref),
            lifecycle_state,
            error: onboarding_result.err().map(|error| error.to_string()),
        }
    }

    fn registration_failure_result_with_secret_cleanup(
        &self,
        switch: &SwitchBootstrapRequest,
        secret_ref: String,
        registration_error: UnderlayError,
    ) -> DeviceInitializationResult {
        if let Err(cleanup_error) = self.secret_store.delete(&secret_ref) {
            return DeviceInitializationResult {
                device_id: switch.device_id.clone(),
                role: switch.role,
                secret_ref: Some(secret_ref),
                lifecycle_state: None,
                error: Some(format!(
                    "{registration_error}; secret cleanup failed for created secret: {cleanup_error}"
                )),
            };
        }

        DeviceInitializationResult {
            device_id: switch.device_id.clone(),
            role: switch.role,
            secret_ref: None,
            lifecycle_state: None,
            error: Some(registration_error.to_string()),
        }
    }
}

pub fn validate_switch_pair(request: &InitializeUnderlaySiteRequest) -> UnderlayResult<()> {
    if request.switches.len() != 2 {
        return Err(UnderlayError::InvalidDeviceState(
            "underlay site initialization requires exactly two switches".into(),
        ));
    }

    let leaf_a_count = request
        .switches
        .iter()
        .filter(|switch| switch.role == DeviceRole::LeafA)
        .count();
    let leaf_b_count = request
        .switches
        .iter()
        .filter(|switch| switch.role == DeviceRole::LeafB)
        .count();

    if leaf_a_count != 1 || leaf_b_count != 1 {
        return Err(UnderlayError::InvalidDeviceState(
            "switch pair must contain exactly one LeafA and one LeafB".into(),
        ));
    }

    if request.switches[0].device_id == request.switches[1].device_id {
        return Err(UnderlayError::InvalidDeviceState(
            "switch pair device_id values must be unique".into(),
        ));
    }

    Ok(())
}

fn summarize_status(
    devices: &[DeviceInitializationResult],
    allow_degraded: bool,
) -> SiteInitializationStatus {
    if devices.iter().any(|device| device.lifecycle_state.is_none()) {
        return SiteInitializationStatus::PartiallyRegistered;
    }

    if devices
        .iter()
        .all(|device| device.lifecycle_state.as_ref() == Some(&DeviceLifecycleState::Ready))
    {
        return SiteInitializationStatus::Ready;
    }

    if allow_degraded
        && devices.iter().all(|device| {
            matches!(
                device.lifecycle_state.as_ref(),
                Some(DeviceLifecycleState::Ready | DeviceLifecycleState::Degraded)
            )
        })
    {
        return SiteInitializationStatus::ReadyWithDegradedDevice;
    }

    SiteInitializationStatus::Failed
}
