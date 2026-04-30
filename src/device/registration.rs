use serde::{Deserialize, Serialize};

use crate::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use crate::model::{DeviceId, DeviceRole, Vendor};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDeviceRequest {
    pub tenant_id: String,
    pub site_id: String,
    pub device_id: DeviceId,
    pub management_ip: String,
    pub management_port: u16,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
    pub role: DeviceRole,
    pub secret_ref: String,
    pub host_key_policy: HostKeyPolicy,
    pub adapter_endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDeviceResponse {
    pub device_id: DeviceId,
    pub lifecycle_state: DeviceLifecycleState,
}

#[derive(Debug, Clone)]
pub struct DeviceRegistrationService {
    inventory: DeviceInventory,
}

impl DeviceRegistrationService {
    pub fn new(inventory: DeviceInventory) -> Self {
        Self { inventory }
    }

    pub fn register(
        &self,
        request: RegisterDeviceRequest,
    ) -> UnderlayResult<RegisterDeviceResponse> {
        validate_register_device_request(&request)?;

        let info = DeviceInfo {
            tenant_id: request.tenant_id,
            site_id: request.site_id,
            id: request.device_id.clone(),
            management_ip: request.management_ip,
            management_port: request.management_port,
            vendor_hint: request.vendor_hint,
            model_hint: request.model_hint,
            role: request.role,
            secret_ref: request.secret_ref,
            host_key_policy: request.host_key_policy,
            adapter_endpoint: request.adapter_endpoint,
            lifecycle_state: DeviceLifecycleState::Pending,
        };

        self.inventory.insert(info)?;

        Ok(RegisterDeviceResponse {
            device_id: request.device_id,
            lifecycle_state: DeviceLifecycleState::Pending,
        })
    }
}

pub fn validate_register_device_request(request: &RegisterDeviceRequest) -> UnderlayResult<()> {
    validate_required("tenant_id", &request.tenant_id)?;
    validate_required("site_id", &request.site_id)?;
    validate_device_id("device_id", &request.device_id)?;
    validate_required("management_ip", &request.management_ip)?;
    validate_nonzero_port("management_port", request.management_port)?;
    validate_required("secret_ref", &request.secret_ref)?;
    validate_adapter_endpoint(&request.adapter_endpoint)?;
    validate_host_key_policy(&request.host_key_policy)?;
    Ok(())
}

pub(crate) fn validate_required(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidDeviceState(format!("{field} is empty")));
    }
    Ok(())
}

pub(crate) fn validate_device_id(field: &str, device_id: &DeviceId) -> UnderlayResult<()> {
    if !device_id.is_canonical() {
        return Err(UnderlayError::InvalidDeviceState(format!(
            "{field} {} is invalid: {}",
            device_id.0,
            DeviceId::canonical_rule()
        )));
    }
    Ok(())
}

pub(crate) fn validate_nonzero_port(field: &str, port: u16) -> UnderlayResult<()> {
    if port == 0 {
        return Err(UnderlayError::InvalidDeviceState(format!(
            "{field} must be non-zero"
        )));
    }
    Ok(())
}

pub(crate) fn validate_adapter_endpoint(adapter_endpoint: &str) -> UnderlayResult<()> {
    validate_required("adapter_endpoint", adapter_endpoint)?;
    tonic::transport::Endpoint::from_shared(adapter_endpoint.to_string()).map_err(|err| {
        UnderlayError::InvalidDeviceState(format!("adapter_endpoint is invalid: {err}"))
    })?;
    Ok(())
}

pub(crate) fn validate_host_key_policy(policy: &HostKeyPolicy) -> UnderlayResult<()> {
    match policy {
        HostKeyPolicy::TrustOnFirstUse => Ok(()),
        HostKeyPolicy::KnownHostsFile { path } => {
            validate_required("host_key_policy.known_hosts_path", path)?;
            if path.contains('\n') || path.contains('\r') {
                return Err(UnderlayError::InvalidDeviceState(
                    "host_key_policy.known_hosts_path must be a single-line path".into(),
                ));
            }
            Ok(())
        }
        HostKeyPolicy::PinnedKey { fingerprint } => {
            validate_required("host_key_policy.pinned_fingerprint", fingerprint)
        }
    }
}
