use serde::{Deserialize, Serialize};

use crate::engine::diff::ChangeSet;
use crate::model::Vendor;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderedConfigFormat {
    NetconfXml,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedConfig {
    pub vendor: Vendor,
    pub format: RenderedConfigFormat,
    pub payload: String,
}

pub trait DeviceConfigRenderer: std::fmt::Debug + Send + Sync {
    fn vendor(&self) -> Vendor;

    fn render_change_set(&self, change_set: &ChangeSet) -> UnderlayResult<RenderedConfig>;
}

#[derive(Debug, Default)]
pub struct HuaweiRenderer;

#[derive(Debug, Default)]
pub struct H3cRenderer;

#[derive(Debug, Default)]
pub struct CiscoRenderer;

#[derive(Debug, Default)]
pub struct RuijieRenderer;

impl DeviceConfigRenderer for HuaweiRenderer {
    fn vendor(&self) -> Vendor {
        Vendor::Huawei
    }

    fn render_change_set(&self, change_set: &ChangeSet) -> UnderlayResult<RenderedConfig> {
        renderer_not_implemented(self.vendor(), change_set)
    }
}

impl DeviceConfigRenderer for H3cRenderer {
    fn vendor(&self) -> Vendor {
        Vendor::H3c
    }

    fn render_change_set(&self, change_set: &ChangeSet) -> UnderlayResult<RenderedConfig> {
        renderer_not_implemented(self.vendor(), change_set)
    }
}

impl DeviceConfigRenderer for CiscoRenderer {
    fn vendor(&self) -> Vendor {
        Vendor::Cisco
    }

    fn render_change_set(&self, change_set: &ChangeSet) -> UnderlayResult<RenderedConfig> {
        renderer_not_implemented(self.vendor(), change_set)
    }
}

impl DeviceConfigRenderer for RuijieRenderer {
    fn vendor(&self) -> Vendor {
        Vendor::Ruijie
    }

    fn render_change_set(&self, change_set: &ChangeSet) -> UnderlayResult<RenderedConfig> {
        renderer_not_implemented(self.vendor(), change_set)
    }
}

pub fn renderer_for_vendor(vendor: Vendor) -> UnderlayResult<Box<dyn DeviceConfigRenderer>> {
    match vendor {
        Vendor::Huawei => Ok(Box::new(HuaweiRenderer)),
        Vendor::H3c => Ok(Box::new(H3cRenderer)),
        Vendor::Cisco => Ok(Box::new(CiscoRenderer)),
        Vendor::Ruijie => Ok(Box::new(RuijieRenderer)),
        Vendor::Unknown => Err(UnderlayError::Internal(
            "cannot select config renderer for unknown vendor".into(),
        )),
    }
}

fn renderer_not_implemented(
    vendor: Vendor,
    change_set: &ChangeSet,
) -> UnderlayResult<RenderedConfig> {
    Err(UnderlayError::Internal(format!(
        "config renderer for {vendor:?} is not implemented yet; device {} has {} pending ops",
        change_set.device_id.0,
        change_set.ops.len()
    )))
}
