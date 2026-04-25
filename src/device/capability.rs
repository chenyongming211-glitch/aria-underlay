use serde::{Deserialize, Serialize};

use crate::model::Vendor;
use crate::tx::TransactionStrategy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    Netconf,
    Napalm,
    Netmiko,
    Cli,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilityProfile {
    pub vendor: Vendor,
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub raw_capabilities: Vec<String>,
    pub supports_netconf: bool,
    pub supports_candidate: bool,
    pub supports_validate: bool,
    pub supports_confirmed_commit: bool,
    pub supports_persist_id: bool,
    pub supports_rollback_on_error: bool,
    pub supports_writable_running: bool,
    pub supported_backends: Vec<BackendKind>,
    pub recommended_strategy: TransactionStrategy,
}

