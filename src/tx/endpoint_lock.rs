use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::model::DeviceId;
use super::lock_strategy::LockAcquisitionPolicy;
use crate::{UnderlayError, UnderlayResult};

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
        let ordered = ordered_device_ids(device_ids);
        let mut guards = Vec::with_capacity(ordered.len());
        for device_id in ordered {
            let lock = self.lock_for(device_id);
            guards.push(lock.lock_owned().await);
        }

        Ok(EndpointWriteGuard { _guards: guards })
    }

    pub async fn acquire_many_with_policy(
        &self,
        device_ids: &[DeviceId],
        policy: &LockAcquisitionPolicy,
    ) -> UnderlayResult<EndpointWriteGuard> {
        let ordered = ordered_device_ids(device_ids);

        let deadline = Instant::now() + Duration::from_secs(policy.max_wait_secs);
        let mut delay = policy.initial_delay();
        let max_delay = Duration::from_secs(policy.max_delay_secs);

        loop {
            if let Some(guard) = self.try_acquire_ordered(&ordered) {
                return Ok(guard);
            }

            if Instant::now() >= deadline {
                return Err(UnderlayError::AdapterOperation {
                    code: "ENDPOINT_LOCK_TIMEOUT".into(),
                    message: format!(
                        "timed out acquiring local endpoint lock for {:?}",
                        ordered
                            .iter()
                            .map(|device_id| device_id.0.as_str())
                            .collect::<Vec<_>>()
                    ),
                    retryable: true,
                    errors: Vec::new(),
                });
            }

            tokio::time::sleep(delay).await;
            delay = std::cmp::min(delay.saturating_mul(2), max_delay);
        }
    }

    fn try_acquire_ordered(&self, ordered: &[DeviceId]) -> Option<EndpointWriteGuard> {
        let mut guards = Vec::with_capacity(ordered.len());
        for device_id in ordered {
            let lock = self.lock_for(device_id.clone());
            match lock.try_lock_owned() {
                Ok(guard) => guards.push(guard),
                Err(_) => return None,
            }
        }

        Some(EndpointWriteGuard { _guards: guards })
    }

    fn lock_for(&self, device_id: DeviceId) -> Arc<Mutex<()>> {
        self.locks
            .entry(device_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn ordered_device_ids(device_ids: &[DeviceId]) -> Vec<DeviceId> {
    let mut ordered = device_ids.to_vec();
    ordered.sort();
    ordered.dedup();
    ordered
}
