use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use uuid::Uuid;

use crate::utils::atomic_file::atomic_write;
use crate::utils::time::now_unix_secs;
use crate::{AdapterErrorDetail, UnderlayError, UnderlayResult};

const DEFAULT_ACTIVE_LEASE_TTL_SECS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveLeaseRecord {
    pub owner_id: String,
    pub token: String,
    pub acquired_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLeaseConfig {
    pub path: PathBuf,
    pub owner_id: String,
    pub ttl_secs: u64,
    pub heartbeat_interval_secs: u64,
}

#[derive(Debug)]
pub struct ActiveLeaseGuard {
    path: PathBuf,
    owner_id: String,
    token: String,
    acquired_at_unix_secs: u64,
    ttl_secs: u64,
    running: Arc<AtomicBool>,
    heartbeat: Option<JoinHandle<()>>,
}

impl ActiveLeaseConfig {
    pub fn new(path: impl Into<PathBuf>, owner_id: impl Into<String>) -> Self {
        let ttl_secs = DEFAULT_ACTIVE_LEASE_TTL_SECS;
        Self {
            path: path.into(),
            owner_id: owner_id.into(),
            ttl_secs,
            heartbeat_interval_secs: heartbeat_interval_for_ttl(ttl_secs),
        }
    }

    pub fn with_ttl_secs(mut self, ttl_secs: u64) -> Self {
        self.ttl_secs = ttl_secs;
        self.heartbeat_interval_secs = heartbeat_interval_for_ttl(ttl_secs);
        self
    }

    pub fn with_heartbeat_interval_secs(mut self, heartbeat_interval_secs: u64) -> Self {
        self.heartbeat_interval_secs = heartbeat_interval_secs;
        self
    }

    fn validate(&self) -> UnderlayResult<()> {
        if self.path.as_os_str().is_empty() {
            return Err(UnderlayError::InvalidIntent(
                "active-passive lease path must not be empty".into(),
            ));
        }
        if self.owner_id.trim().is_empty() {
            return Err(UnderlayError::InvalidIntent(
                "active-passive lease owner_id must not be empty".into(),
            ));
        }
        if self.ttl_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "active-passive lease ttl_secs must be greater than zero".into(),
            ));
        }
        if self.heartbeat_interval_secs == 0 {
            return Err(UnderlayError::InvalidIntent(
                "active-passive lease heartbeat_interval_secs must be greater than zero".into(),
            ));
        }
        if self.heartbeat_interval_secs > self.ttl_secs {
            return Err(UnderlayError::InvalidIntent(
                "active-passive lease heartbeat_interval_secs must not exceed ttl_secs".into(),
            ));
        }
        Ok(())
    }
}

impl ActiveLeaseGuard {
    pub fn acquire(config: ActiveLeaseConfig) -> UnderlayResult<Self> {
        config.validate()?;
        create_parent_dir(&config.path)?;

        let token = Uuid::new_v4().to_string();
        let acquired_at_unix_secs = now_unix_secs();
        let record = ActiveLeaseRecord {
            owner_id: config.owner_id.clone(),
            token: token.clone(),
            acquired_at_unix_secs,
            updated_at_unix_secs: acquired_at_unix_secs,
        };

        match create_lease_file(&config.path, &record) {
            Ok(()) => Self::from_acquired(config, token, acquired_at_unix_secs),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let current = read_lease_record(&config.path)?;
                if !lease_is_stale(&current, config.ttl_secs) {
                    return Err(active_lease_error(
                        "HA_LEASE_HELD",
                        format!(
                            "active-passive lease {:?} is held by {}",
                            config.path, current.owner_id
                        ),
                        true,
                    ));
                }
                remove_stale_lease(&config.path)?;
                create_lease_file(&config.path, &record).map_err(active_lease_io_error)?;
                Self::from_acquired(config, token, acquired_at_unix_secs)
            }
            Err(err) => Err(active_lease_io_error(err)),
        }
    }

    pub fn ensure_current(&self) -> UnderlayResult<()> {
        let record = read_lease_record(&self.path)?;
        if record.token != self.token {
            return Err(active_lease_error(
                "HA_LEASE_LOST",
                format!(
                    "active-passive lease {:?} moved from {} to {}",
                    self.path, self.owner_id, record.owner_id
                ),
                true,
            ));
        }
        if lease_is_stale(&record, self.ttl_secs) {
            return Err(active_lease_error(
                "HA_LEASE_STALE",
                format!("active-passive lease {:?} heartbeat is stale", self.path),
                true,
            ));
        }
        Ok(())
    }

    pub fn record(&self) -> ActiveLeaseRecord {
        ActiveLeaseRecord {
            owner_id: self.owner_id.clone(),
            token: self.token.clone(),
            acquired_at_unix_secs: self.acquired_at_unix_secs,
            updated_at_unix_secs: now_unix_secs(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn from_acquired(
        config: ActiveLeaseConfig,
        token: String,
        acquired_at_unix_secs: u64,
    ) -> UnderlayResult<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let heartbeat = match spawn_heartbeat(
            config.path.clone(),
            config.owner_id.clone(),
            token.clone(),
            acquired_at_unix_secs,
            config.heartbeat_interval_secs,
            running.clone(),
        ) {
            Ok(heartbeat) => heartbeat,
            Err(err) => {
                release_lease_if_current(&config.path, &token);
                return Err(err);
            }
        };
        Ok(Self {
            path: config.path,
            owner_id: config.owner_id,
            token,
            acquired_at_unix_secs,
            ttl_secs: config.ttl_secs,
            running,
            heartbeat: Some(heartbeat),
        })
    }
}

impl Drop for ActiveLeaseGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.thread().unpark();
            let _ = heartbeat.join();
        }
        release_lease_if_current(&self.path, &self.token);
    }
}

fn spawn_heartbeat(
    path: PathBuf,
    owner_id: String,
    token: String,
    acquired_at_unix_secs: u64,
    interval_secs: u64,
    running: Arc<AtomicBool>,
) -> UnderlayResult<JoinHandle<()>> {
    thread::Builder::new()
        .name("aria-underlay-active-lease-heartbeat".into())
        .spawn(move || {
            let interval = Duration::from_secs(interval_secs);
            while running.load(Ordering::Acquire) {
                thread::park_timeout(interval);
                if !running.load(Ordering::Acquire) {
                    break;
                }
                let _ = refresh_lease_record(&path, &owner_id, &token, acquired_at_unix_secs);
            }
        })
        .map_err(|err| UnderlayError::Internal(format!("spawn active lease heartbeat: {err}")))
}

fn refresh_lease_record(
    path: &Path,
    owner_id: &str,
    token: &str,
    acquired_at_unix_secs: u64,
) -> UnderlayResult<()> {
    let current = read_lease_record(path)?;
    if current.token != token {
        return Err(active_lease_error(
            "HA_LEASE_LOST",
            format!("active-passive lease {:?} is no longer held by this process", path),
            true,
        ));
    }
    let updated = ActiveLeaseRecord {
        owner_id: owner_id.into(),
        token: token.into(),
        acquired_at_unix_secs,
        updated_at_unix_secs: now_unix_secs(),
    };
    write_lease_record(path, &updated)
}

fn create_lease_file(path: &Path, record: &ActiveLeaseRecord) -> std::io::Result<()> {
    let payload = serde_json::to_vec_pretty(record)
        .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    std::io::Write::write_all(&mut file, &payload)?;
    std::io::Write::write_all(&mut file, b"\n")?;
    std::io::Write::flush(&mut file)
}

fn write_lease_record(path: &Path, record: &ActiveLeaseRecord) -> UnderlayResult<()> {
    let payload = serde_json::to_vec_pretty(record)
        .map_err(|err| UnderlayError::Internal(format!("serialize active lease: {err}")))?;
    atomic_write(path, &payload, active_lease_io_error)
}

fn read_lease_record(path: &Path) -> UnderlayResult<ActiveLeaseRecord> {
    let payload = fs::read(path).map_err(active_lease_io_error)?;
    serde_json::from_slice(&payload).map_err(|err| {
        UnderlayError::Internal(format!("parse active-passive lease {:?}: {err}", path))
    })
}

fn create_parent_dir(path: &Path) -> UnderlayResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(active_lease_io_error)?;
        }
    }
    Ok(())
}

fn remove_stale_lease(path: &Path) -> UnderlayResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(active_lease_io_error(err)),
    }
}

fn release_lease_if_current(path: &Path, token: &str) {
    if let Ok(record) = read_lease_record(path) {
        if record.token == token {
            let _ = fs::remove_file(path);
        }
    }
}

fn lease_is_stale(record: &ActiveLeaseRecord, ttl_secs: u64) -> bool {
    now_unix_secs().saturating_sub(record.updated_at_unix_secs) > ttl_secs
}

fn heartbeat_interval_for_ttl(ttl_secs: u64) -> u64 {
    (ttl_secs / 3).max(1)
}

fn active_lease_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("active-passive lease io error: {err}"))
}

fn active_lease_error(
    code: impl Into<String>,
    message: impl Into<String>,
    retryable: bool,
) -> UnderlayError {
    let code = code.into();
    let message = message.into();
    UnderlayError::AdapterOperation {
        code: code.clone(),
        message: message.clone(),
        retryable,
        errors: vec![AdapterErrorDetail { code, message }],
    }
}
