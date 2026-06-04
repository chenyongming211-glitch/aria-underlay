use std::collections::BTreeSet;
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
    _guards: Vec<OwnedMutexGuard<()>>,
}

impl DomainApplyLockTable {
    pub async fn acquire(&self, domain_id: &str) -> UnderlayResult<DomainApplyGuard> {
        self.acquire_many([format!(
            "domain:{}",
            normalize_lock_component("domain_id", domain_id)?
        )])
        .await
    }

    pub async fn acquire_many<I, S>(&self, keys: I) -> UnderlayResult<DomainApplyGuard>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let keys = normalize_scope_keys(keys)?;
        let mut guards = Vec::with_capacity(keys.len());
        for key in keys {
            let lock = self.lock_for(key);
            guards.push(lock.lock_owned().await);
        }
        Ok(DomainApplyGuard { _guards: guards })
    }

    fn lock_for(&self, domain_id: String) -> Arc<Mutex<()>> {
        self.locks
            .entry(domain_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn normalize_scope_keys<I, S>(keys: I) -> UnderlayResult<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let keys = keys
        .into_iter()
        .map(|key| normalize_scope_key(key.as_ref()))
        .collect::<UnderlayResult<BTreeSet<_>>>()?;

    if keys.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "apply lock scope must include at least one key".into(),
        ));
    }

    Ok(keys.into_iter().collect())
}

fn normalize_scope_key(value: &str) -> UnderlayResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "apply lock scope key must not be empty".into(),
        ));
    }
    Ok(value.to_string())
}

fn normalize_lock_component(field: &str, value: &str) -> UnderlayResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(UnderlayError::InvalidIntent(format!("{field} must not be empty")));
    }
    if value.contains(':') {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} must not contain ':'"
        )));
    }
    Ok(value.to_string())
}
