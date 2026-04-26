use std::collections::BTreeMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::model::{DeviceId, InterfaceConfig, VlanConfig};
use crate::planner::device_plan::DeviceDesiredState;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceShadowState {
    pub device_id: DeviceId,
    pub revision: u64,
    pub vlans: BTreeMap<u16, VlanConfig>,
    pub interfaces: BTreeMap<String, InterfaceConfig>,
    pub warnings: Vec<String>,
}

impl DeviceShadowState {
    pub fn from_desired(desired: &DeviceDesiredState, revision: u64) -> Self {
        Self {
            device_id: desired.device_id.clone(),
            revision,
            vlans: desired.vlans.clone(),
            interfaces: desired.interfaces.clone(),
            warnings: Vec::new(),
        }
    }

    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }
}

pub trait ShadowStateStore: std::fmt::Debug + Send + Sync {
    fn get(&self, device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>>;
    fn put(&self, state: DeviceShadowState) -> UnderlayResult<DeviceShadowState>;
    fn remove(&self, device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>>;
    fn list(&self) -> UnderlayResult<Vec<DeviceShadowState>>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryShadowStateStore {
    inner: DashMap<DeviceId, DeviceShadowState>,
}

impl ShadowStateStore for InMemoryShadowStateStore {
    fn get(&self, device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        Ok(self.inner.get(device_id).map(|entry| entry.value().clone()))
    }

    fn put(&self, mut state: DeviceShadowState) -> UnderlayResult<DeviceShadowState> {
        let next_revision = self
            .inner
            .get(&state.device_id)
            .map(|entry| entry.revision.saturating_add(1))
            .unwrap_or_else(|| state.revision.max(1));
        state.revision = next_revision;
        self.inner.insert(state.device_id.clone(), state.clone());
        Ok(state)
    }

    fn remove(&self, device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        Ok(self.inner.remove(device_id).map(|(_, state)| state))
    }

    fn list(&self) -> UnderlayResult<Vec<DeviceShadowState>> {
        let mut states = self
            .inner
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        states.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        Ok(states)
    }
}

pub fn missing_shadow_state(device_id: &DeviceId) -> UnderlayError {
    UnderlayError::InvalidDeviceState(format!(
        "missing shadow state for device {}",
        device_id.0
    ))
}
