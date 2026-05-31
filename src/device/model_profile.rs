use serde::{Deserialize, Serialize};

use crate::model::Vendor;
use crate::proto::adapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelProtocol {
    OpenConfigGnmi,
    OpenConfigNetconf,
    VendorNativeYang,
    VendorCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteReadiness {
    ReadOnly,
    WriteSafe,
    WriteRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteDecision {
    AllowedStandardModel,
    AllowedVendorNative,
    ReadOnlyOnly,
    RejectedUnsafeTransaction,
    RejectedMissingPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPathSupport {
    pub protocol: ModelProtocol,
    pub model: String,
    pub revision: Option<String>,
    pub path: String,
    pub readable: bool,
    pub writable: bool,
    pub verified_on_device: bool,
    pub deviations: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceModelProfile {
    pub profile_id: String,
    pub vendor: Vendor,
    pub model: String,
    pub os_version: String,
    pub paths: Vec<ModelPathSupport>,
    pub pbr_write_readiness: WriteReadiness,
    pub bgp_write_readiness: WriteReadiness,
    pub rejection_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSupport {
    pub feature: String,
    pub required_paths: Vec<ModelPathSupport>,
    pub requires_candidate: bool,
    pub requires_validate: bool,
    pub supports_candidate: bool,
    pub supports_validate: bool,
}

impl FeatureSupport {
    pub fn write_decision(&self) -> WriteDecision {
        if (self.requires_candidate && !self.supports_candidate)
            || (self.requires_validate && !self.supports_validate)
        {
            return WriteDecision::RejectedUnsafeTransaction;
        }

        let Some(best_path) = self
            .required_paths
            .iter()
            .find(|path| path.readable && path.writable && path.verified_on_device)
        else {
            return WriteDecision::RejectedMissingPath;
        };

        match best_path.protocol {
            ModelProtocol::OpenConfigGnmi | ModelProtocol::OpenConfigNetconf => {
                WriteDecision::AllowedStandardModel
            }
            ModelProtocol::VendorNativeYang => WriteDecision::AllowedVendorNative,
            ModelProtocol::VendorCli => WriteDecision::ReadOnlyOnly,
        }
    }
}

impl DeviceModelProfile {
    pub fn from_proto(proto: adapter::DeviceModelProfile) -> Self {
        Self {
            profile_id: proto.profile_id,
            vendor: vendor_from_proto(proto.vendor),
            model: proto.model,
            os_version: proto.os_version,
            paths: proto.paths.into_iter().map(ModelPathSupport::from_proto).collect(),
            pbr_write_readiness: WriteReadiness::from_proto(proto.pbr_write_readiness),
            bgp_write_readiness: WriteReadiness::from_proto(proto.bgp_write_readiness),
            rejection_reasons: proto.rejection_reasons,
        }
    }
}

impl ModelPathSupport {
    pub fn from_proto(proto: adapter::ModelPathSupport) -> Self {
        Self {
            protocol: ModelProtocol::from_proto(proto.protocol),
            model: proto.model,
            revision: empty_to_none(proto.revision),
            path: proto.path,
            readable: proto.readable,
            writable: proto.writable,
            verified_on_device: proto.verified_on_device,
            deviations: proto.deviations,
            notes: proto.notes,
        }
    }
}

impl ModelProtocol {
    fn from_proto(value: i32) -> Self {
        match adapter::ModelProtocol::try_from(value)
            .unwrap_or(adapter::ModelProtocol::Unspecified)
        {
            adapter::ModelProtocol::OpenconfigGnmi => Self::OpenConfigGnmi,
            adapter::ModelProtocol::OpenconfigNetconf => Self::OpenConfigNetconf,
            adapter::ModelProtocol::VendorNativeYang => Self::VendorNativeYang,
            adapter::ModelProtocol::VendorCli => Self::VendorCli,
            adapter::ModelProtocol::Unspecified => Self::VendorCli,
        }
    }
}

impl WriteReadiness {
    fn from_proto(value: i32) -> Self {
        match adapter::WriteReadiness::try_from(value)
            .unwrap_or(adapter::WriteReadiness::Unspecified)
        {
            adapter::WriteReadiness::ReadOnly => Self::ReadOnly,
            adapter::WriteReadiness::WriteSafe => Self::WriteSafe,
            adapter::WriteReadiness::WriteRejected | adapter::WriteReadiness::Unspecified => {
                Self::WriteRejected
            }
        }
    }
}

fn vendor_from_proto(value: i32) -> Vendor {
    match adapter::Vendor::try_from(value).unwrap_or(adapter::Vendor::Unknown) {
        adapter::Vendor::Huawei => Vendor::Huawei,
        adapter::Vendor::H3c => Vendor::H3c,
        adapter::Vendor::Cisco => Vendor::Cisco,
        adapter::Vendor::Ruijie => Vendor::Ruijie,
        _ => Vendor::Unknown,
    }
}

fn empty_to_none(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
