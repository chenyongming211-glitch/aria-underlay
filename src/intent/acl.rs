use serde::{Deserialize, Serialize};

use crate::model::{AclDirection, AclRule, DeviceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclIntent {
    pub acl_id: u16,
    pub name: Option<String>,
    pub description: Option<String>,
    pub rules: Vec<AclRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclBindingIntent {
    pub device_id: DeviceId,
    pub interface_name: String,
    pub direction: AclDirection,
    pub acl_id: u16,
}
