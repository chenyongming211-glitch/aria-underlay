use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::api::request::ApplyDomainIntentRequest;
use crate::api::response::{ApplyIntentResponse, ApplyStatus};
use crate::intent::{UnderlayDomainIntent, UnderlayTopology};
use crate::model::DeviceId;
use crate::utils::atomic_file::atomic_write;
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainApplyRecord {
    pub request: ApplyDomainIntentRequest,
    pub response: ApplyIntentResponse,
    pub domain_id: String,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainApplyCompensationPlan {
    pub original_request_id: String,
    pub original_trace_id: String,
    pub domain_id: String,
    pub status: ApplyStatus,
    pub retryable_failed: Vec<DeviceId>,
    pub requires_recovery: Vec<DeviceId>,
    pub completed: Vec<DeviceId>,
}

pub trait DomainApplyRecordStore: std::fmt::Debug + Send + Sync {
    fn put(&self, record: &DomainApplyRecord) -> UnderlayResult<()>;
    fn get(&self, request_id: &str) -> UnderlayResult<Option<DomainApplyRecord>>;
}

#[derive(Debug, Default)]
pub struct InMemoryDomainApplyRecordStore {
    records: Mutex<BTreeMap<String, DomainApplyRecord>>,
}

#[derive(Debug)]
pub struct JsonFileDomainApplyRecordStore {
    root: PathBuf,
    lock: Mutex<()>,
}

impl DomainApplyRecord {
    pub fn new(request: ApplyDomainIntentRequest, response: ApplyIntentResponse) -> Self {
        let now = now_unix_secs();
        Self {
            domain_id: request.intent.domain_id.clone(),
            request,
            response,
            created_at_unix_secs: now,
            updated_at_unix_secs: now,
        }
    }
}

impl DomainApplyCompensationPlan {
    pub fn from_record(record: &DomainApplyRecord) -> Self {
        let mut retryable_failed = Vec::new();
        let mut requires_recovery = Vec::new();
        let mut completed = Vec::new();

        for result in &record.response.device_results {
            match &result.status {
                ApplyStatus::Failed | ApplyStatus::RolledBack => {
                    retryable_failed.push(result.device_id.clone());
                }
                ApplyStatus::InDoubt => {
                    requires_recovery.push(result.device_id.clone());
                }
                ApplyStatus::Success
                | ApplyStatus::SuccessWithWarning
                | ApplyStatus::NoOpSuccess => {
                    completed.push(result.device_id.clone());
                }
                ApplyStatus::PartialSuccess => {}
            }
        }

        Self {
            original_request_id: record.request.request_id.clone(),
            original_trace_id: record
                .request
                .trace_id
                .clone()
                .unwrap_or_else(|| record.request.request_id.clone()),
            domain_id: record.domain_id.clone(),
            status: record.response.status.clone(),
            retryable_failed,
            requires_recovery,
            completed,
        }
    }
}

impl InMemoryDomainApplyRecordStore {
    pub fn put(&self, record: &DomainApplyRecord) -> UnderlayResult<()> {
        <Self as DomainApplyRecordStore>::put(self, record)
    }

    pub fn get(&self, request_id: &str) -> UnderlayResult<Option<DomainApplyRecord>> {
        <Self as DomainApplyRecordStore>::get(self, request_id)
    }
}

impl DomainApplyRecordStore for InMemoryDomainApplyRecordStore {
    fn put(&self, record: &DomainApplyRecord) -> UnderlayResult<()> {
        self.records
            .lock()
            .map_err(|_| UnderlayError::Internal("domain apply record mutex poisoned".into()))?
            .insert(record.request.request_id.clone(), record.clone());
        Ok(())
    }

    fn get(&self, request_id: &str) -> UnderlayResult<Option<DomainApplyRecord>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| UnderlayError::Internal("domain apply record mutex poisoned".into()))?
            .get(request_id)
            .cloned())
    }
}

impl JsonFileDomainApplyRecordStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, record: &DomainApplyRecord) -> UnderlayResult<()> {
        <Self as DomainApplyRecordStore>::put(self, record)
    }

    pub fn get(&self, request_id: &str) -> UnderlayResult<Option<DomainApplyRecord>> {
        <Self as DomainApplyRecordStore>::get(self, request_id)
    }

    fn path_for(&self, request_id: &str) -> UnderlayResult<PathBuf> {
        validate_domain_apply_request_id(request_id)?;
        Ok(self.root.join(format!("{}.json", hex_encode(request_id))))
    }
}

impl DomainApplyRecordStore for JsonFileDomainApplyRecordStore {
    fn put(&self, record: &DomainApplyRecord) -> UnderlayResult<()> {
        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("domain apply record file mutex poisoned".into())
        })?;
        let path = self.path_for(&record.request.request_id)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(domain_apply_record_io_error)?;
        }
        let payload = serde_json::to_vec_pretty(record).map_err(|err| {
            UnderlayError::Internal(format!("serialize domain apply record: {err}"))
        })?;
        atomic_write(&path, &payload, domain_apply_record_io_error)
    }

    fn get(&self, request_id: &str) -> UnderlayResult<Option<DomainApplyRecord>> {
        let _guard = self.lock.lock().map_err(|_| {
            UnderlayError::Internal("domain apply record file mutex poisoned".into())
        })?;
        let path = self.path_for(request_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let payload = fs::read(&path).map_err(domain_apply_record_io_error)?;
        serde_json::from_slice(&payload)
            .map(Some)
            .map_err(|err| {
                UnderlayError::Internal(format!("parse domain apply record {:?}: {err}", path))
            })
    }
}

pub fn select_retryable_failed_endpoints(
    record: &DomainApplyRecord,
    explicit_endpoint_ids: &[DeviceId],
) -> UnderlayResult<Vec<DeviceId>> {
    let plan = DomainApplyCompensationPlan::from_record(record);
    let retryable = plan
        .retryable_failed
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let selected = if explicit_endpoint_ids.is_empty() {
        plan.retryable_failed
    } else {
        let mut selected = Vec::with_capacity(explicit_endpoint_ids.len());
        for endpoint_id in explicit_endpoint_ids {
            if !retryable.contains(endpoint_id) {
                return Err(UnderlayError::InvalidIntent(format!(
                    "endpoint {} is not a terminal failed endpoint for original request {}",
                    endpoint_id.0, record.request.request_id
                )));
            }
            selected.push(endpoint_id.clone());
        }
        selected.sort();
        selected.dedup();
        selected
    };

    if selected.is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "original request {} has no terminal failed endpoints to retry",
            record.request.request_id
        )));
    }
    Ok(selected)
}

pub fn filter_domain_intent_to_endpoints(
    intent: &UnderlayDomainIntent,
    endpoint_ids: &[DeviceId],
) -> UnderlayResult<UnderlayDomainIntent> {
    if endpoint_ids.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "retry endpoint list must not be empty".into(),
        ));
    }

    let selected_endpoints = endpoint_ids
        .iter()
        .map(|device_id| device_id.0.clone())
        .collect::<BTreeSet<_>>();
    let existing_endpoints = intent
        .endpoints
        .iter()
        .map(|endpoint| endpoint.endpoint_id.clone())
        .collect::<BTreeSet<_>>();
    for endpoint_id in &selected_endpoints {
        if !existing_endpoints.contains(endpoint_id) {
            return Err(UnderlayError::InvalidIntent(format!(
                "endpoint {endpoint_id} does not exist in original domain intent"
            )));
        }
    }

    let endpoints = intent
        .endpoints
        .iter()
        .filter(|endpoint| selected_endpoints.contains(&endpoint.endpoint_id))
        .cloned()
        .collect::<Vec<_>>();
    let members = intent
        .members
        .iter()
        .filter(|member| selected_endpoints.contains(&member.management_endpoint_id))
        .cloned()
        .collect::<Vec<_>>();
    let selected_members = members
        .iter()
        .map(|member| DeviceId(member.member_id.clone()))
        .collect::<BTreeSet<_>>();

    let topology = if endpoints.len() == 1 {
        UnderlayTopology::StackSingleManagementIp
    } else {
        intent.topology
    };

    Ok(UnderlayDomainIntent {
        domain_id: intent.domain_id.clone(),
        topology,
        endpoints,
        members,
        vlans: intent.vlans.clone(),
        interfaces: intent
            .interfaces
            .iter()
            .filter(|interface| selected_members.contains(&interface.device_id))
            .cloned()
            .collect(),
        acls: intent.acls.clone(),
        acl_bindings: intent
            .acl_bindings
            .iter()
            .filter(|binding| selected_members.contains(&binding.device_id))
            .cloned()
            .collect(),
        delete_vlan_ids: intent.delete_vlan_ids.clone(),
        delete_interfaces: intent
            .delete_interfaces
            .iter()
            .filter(|interface| selected_members.contains(&interface.device_id))
            .cloned()
            .collect(),
        delete_acl_ids: intent.delete_acl_ids.clone(),
        delete_acl_bindings: intent
            .delete_acl_bindings
            .iter()
            .filter(|binding| selected_members.contains(&binding.device_id))
            .cloned()
            .collect(),
        bgp_processes: intent
            .bgp_processes
            .iter()
            .filter(|process| selected_members.contains(&process.device_id))
            .cloned()
            .collect(),
        bgp_neighbors: intent
            .bgp_neighbors
            .iter()
            .filter(|neighbor| selected_members.contains(&neighbor.device_id))
            .cloned()
            .collect(),
        delete_bgp_processes: intent
            .delete_bgp_processes
            .iter()
            .filter(|process| selected_members.contains(&process.device_id))
            .cloned()
            .collect(),
        delete_bgp_neighbors: intent
            .delete_bgp_neighbors
            .iter()
            .filter(|neighbor| selected_members.contains(&neighbor.device_id))
            .cloned()
            .collect(),
    })
}

fn validate_domain_apply_request_id(request_id: &str) -> UnderlayResult<()> {
    if request_id.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "domain apply request_id {request_id:?} is invalid for file store"
        )));
    }
    Ok(())
}

fn hex_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for &byte in value.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn domain_apply_record_io_error(err: std::io::Error) -> UnderlayError {
    UnderlayError::Internal(format!("domain apply record io error: {err}"))
}
