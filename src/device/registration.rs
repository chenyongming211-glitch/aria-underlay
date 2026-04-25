use serde::{Deserialize, Serialize};

use crate::device::{DeviceInfo, DeviceInventory, DeviceLifecycleState, HostKeyPolicy};
use crate::model::{DeviceId, DeviceRole, Vendor};
use crate::UnderlayResult;

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

    pub fn register(&self, request: RegisterDeviceRequest) -> UnderlayResult<RegisterDeviceResponse> {
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

