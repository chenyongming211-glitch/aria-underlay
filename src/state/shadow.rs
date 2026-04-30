use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::model::{DeviceId, InterfaceConfig, VlanConfig};
use crate::planner::device_plan::DeviceDesiredState;
use crate::utils::atomic_file::atomic_write;
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

#[derive(Debug, Clone)]
pub struct JsonFileShadowStateStore {
    root: PathBuf,
    locks: Arc<DashMap<DeviceId, Arc<Mutex<()>>>>,
}

impl JsonFileShadowStateStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            locks: Arc::new(DashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, device_id: &DeviceId) -> UnderlayResult<PathBuf> {
        validate_shadow_device_id(device_id)?;
        Ok(self.root.join(format!("{}.json", device_id.0)))
    }

    fn lock_for(&self, device_id: &DeviceId) -> Arc<Mutex<()>> {
        self.locks
            .entry(device_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .value()
            .clone()
    }
}

impl ShadowStateStore for JsonFileShadowStateStore {
    fn get(&self, device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        let path = self.path_for(device_id)?;
        if !path.exists() {
            return Ok(None);
        }
        read_shadow_state(&path).map(Some)
    }

    fn put(&self, mut state: DeviceShadowState) -> UnderlayResult<DeviceShadowState> {
        let lock = self.lock_for(&state.device_id);
        let _guard = lock
            .lock()
            .map_err(|_| UnderlayError::Internal("shadow state mutex poisoned".into()))?;

        let next_revision = self
            .get(&state.device_id)?
            .map(|current| current.revision.saturating_add(1))
            .unwrap_or_else(|| state.revision.max(1));
        state.revision = next_revision;

        let path = self.path_for(&state.device_id)?;
        let payload = serde_json::to_vec_pretty(&state)
            .map_err(|err| UnderlayError::Internal(format!("serialize shadow state: {err}")))?;

        atomic_write(&path, &payload, shadow_io_error)?;
        Ok(state)
    }

    fn remove(&self, device_id: &DeviceId) -> UnderlayResult<Option<DeviceShadowState>> {
        let lock = self.lock_for(device_id);
        let _guard = lock
            .lock()
            .map_err(|_| UnderlayError::Internal("shadow state mutex poisoned".into()))?;
        let path = self.path_for(device_id)?;
        if !path.exists() {
            return Ok(None);
        }

        let state = read_shadow_state(&path)?;
        fs::remove_file(&path).map_err(shadow_io_error)?;
        Ok(Some(state))
    }

    fn list(&self) -> UnderlayResult<Vec<DeviceShadowState>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut states = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(shadow_io_error)? {
            let path = entry.map_err(shadow_io_error)?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            states.push(read_shadow_state(&path)?);
        }
        states.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        Ok(states)
    }
}

fn read_shadow_state(path: &Path) -> UnderlayResult<DeviceShadowState> {
    let payload = fs::read(path).map_err(shadow_io_error)?;
    serde_json::from_slice(&payload)
        .map_err(|err| UnderlayError::Internal(format!("parse shadow state {:?}: {err}", path)))
}

fn validate_shadow_device_id(device_id: &DeviceId) -> UnderlayResult<()> {
    if !device_id.is_canonical() {
        return Err(UnderlayError::InvalidDeviceState(format!(
            "device_id {} is invalid for file shadow store: {}",
            device_id.0,
            DeviceId::canonical_rule()
        )));
    }
    Ok(())
}

fn shadow_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("shadow state io error: {err}"))
}

pub fn missing_shadow_state(device_id: &DeviceId) -> UnderlayError {
    UnderlayError::InvalidDeviceState(format!(
        "missing shadow state for device {}",
        device_id.0
    ))
}
