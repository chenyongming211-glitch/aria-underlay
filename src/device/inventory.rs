use std::sync::Arc;

use dashmap::DashMap;

use crate::device::{DeviceCapabilityProfile, DeviceInfo, DeviceLifecycleState};
use crate::model::DeviceId;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct ManagedDevice {
    pub info: DeviceInfo,
    pub capability: Option<DeviceCapabilityProfile>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceInventory {
    inner: Arc<DashMap<DeviceId, ManagedDevice>>,
}

impl DeviceInventory {
    pub fn insert(&self, info: DeviceInfo) -> UnderlayResult<()> {
        if self.inner.contains_key(&info.id) {
            return Err(UnderlayError::DeviceAlreadyExists(info.id.0));
        }

        self.inner.insert(
            info.id.clone(),
            ManagedDevice {
                info,
                capability: None,
            },
        );
        Ok(())
    }

    pub fn get(&self, device_id: &DeviceId) -> UnderlayResult<ManagedDevice> {
        self.inner
            .get(device_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| UnderlayError::DeviceNotFound(device_id.0.clone()))
    }

    pub fn set_state(
        &self,
        device_id: &DeviceId,
        state: DeviceLifecycleState,
    ) -> UnderlayResult<()> {
        let mut entry = self
            .inner
            .get_mut(device_id)
            .ok_or_else(|| UnderlayError::DeviceNotFound(device_id.0.clone()))?;
        entry.info.lifecycle_state = state;
        Ok(())
    }

    pub fn set_capability(
        &self,
        device_id: &DeviceId,
        capability: DeviceCapabilityProfile,
    ) -> UnderlayResult<()> {
        let mut entry = self
            .inner
            .get_mut(device_id)
            .ok_or_else(|| UnderlayError::DeviceNotFound(device_id.0.clone()))?;
        entry.capability = Some(capability);
        Ok(())
    }
}

