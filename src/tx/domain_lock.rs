use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Default)]
pub struct DomainApplyLockTable {
    locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

#[derive(Debug)]
pub struct DomainApplyGuard {
    _guard: OwnedMutexGuard<()>,
}

impl DomainApplyLockTable {
    pub async fn acquire(&self, domain_id: &str) -> UnderlayResult<DomainApplyGuard> {
        let key = domain_lock_key(domain_id)?;
        let lock = self.lock_for(key);
        Ok(DomainApplyGuard {
            _guard: lock.lock_owned().await,
        })
    }

    fn lock_for(&self, domain_id: String) -> Arc<Mutex<()>> {
        self.locks
            .entry(domain_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn domain_lock_key(domain_id: &str) -> UnderlayResult<String> {
    let key = domain_id.trim();
    if key.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "underlay domain_id must not be empty".into(),
        ));
    }
    Ok(key.to_string())
}
