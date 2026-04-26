use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::model::DeviceId;
use crate::UnderlayResult;

#[derive(Debug, Clone, Default)]
pub struct EndpointLockTable {
    locks: Arc<DashMap<DeviceId, Arc<Mutex<()>>>>,
}

#[derive(Debug)]
pub struct EndpointWriteGuard {
    _guards: Vec<OwnedMutexGuard<()>>,
}

impl EndpointLockTable {
    pub async fn acquire_many(&self, device_ids: &[DeviceId]) -> UnderlayResult<EndpointWriteGuard> {
        let mut ordered = device_ids.to_vec();
        ordered.sort();
        ordered.dedup();

        let mut guards = Vec::with_capacity(ordered.len());
        for device_id in ordered {
            let lock = self
                .locks
                .entry(device_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone();
            guards.push(lock.lock_owned().await);
        }

        Ok(EndpointWriteGuard { _guards: guards })
    }
}
