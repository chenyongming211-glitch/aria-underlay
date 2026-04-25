use crate::adapter_client::AdapterClient;
use crate::device::{DeviceInventory, DeviceLifecycleState};
use crate::model::DeviceId;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct DeviceOnboardingService {
    inventory: DeviceInventory,
}

impl DeviceOnboardingService {
    pub fn new(inventory: DeviceInventory) -> Self {
        Self { inventory }
    }

    pub async fn onboard_device(&self, device_id: DeviceId) -> UnderlayResult<DeviceLifecycleState> {
        let managed = self.inventory.get(&device_id)?;

        match managed.info.lifecycle_state {
            DeviceLifecycleState::Pending
            | DeviceLifecycleState::Unreachable
            | DeviceLifecycleState::AuthFailed
            | DeviceLifecycleState::Unsupported
            | DeviceLifecycleState::Degraded => {}
            other => {
                return Err(UnderlayError::InvalidDeviceState(format!(
                    "cannot onboard device from state {other:?}"
                )));
            }
        }

        self.inventory
            .set_state(&device_id, DeviceLifecycleState::Probing)?;

        let mut client = match AdapterClient::connect(managed.info.adapter_endpoint.clone()).await {
            Ok(client) => client,
            Err(err) => {
                let state = lifecycle_state_for_onboarding_error(&err);
                self.inventory.set_state(&device_id, state.clone())?;
                return Err(err);
            }
        };

        let capability = match client.get_capabilities(&managed.info).await {
            Ok(capability) => capability,
            Err(err) => {
                let state = lifecycle_state_for_onboarding_error(&err);
                self.inventory.set_state(&device_id, state.clone())?;
                return Err(err);
            }
        };

        let state = if capability.recommended_strategy.is_supported() {
            if capability.recommended_strategy.is_degraded() {
                DeviceLifecycleState::Degraded
            } else {
                DeviceLifecycleState::Ready
            }
        } else if capability.supports_netconf || !capability.supported_backends.is_empty() {
            DeviceLifecycleState::Unsupported
        } else {
            DeviceLifecycleState::Unreachable
        };

        self.inventory.set_capability(&device_id, capability)?;
        self.inventory.set_state(&device_id, state.clone())?;
        Ok(state)
    }
}

pub fn lifecycle_state_for_onboarding_error(error: &UnderlayError) -> DeviceLifecycleState {
    match error {
        UnderlayError::AdapterOperation { code, .. } if code == "AUTH_FAILED" => {
            DeviceLifecycleState::AuthFailed
        }
        UnderlayError::AdapterOperation { code, .. } if code == "DEVICE_UNREACHABLE" => {
            DeviceLifecycleState::Unreachable
        }
        UnderlayError::AdapterOperation { code, .. } if code == "UNSUPPORTED_DEVICE" => {
            DeviceLifecycleState::Unsupported
        }
        UnderlayError::AdapterOperation { retryable, .. } if *retryable => {
            DeviceLifecycleState::Unreachable
        }
        UnderlayError::AdapterTransport(_) => DeviceLifecycleState::Unreachable,
        _ => DeviceLifecycleState::Unsupported,
    }
}
