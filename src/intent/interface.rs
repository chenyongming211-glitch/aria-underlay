use serde::{Deserialize, Serialize};

use crate::model::{AdminState, DeviceId, PortMode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceIntent {
    pub device_id: DeviceId,
    pub name: String,
    pub admin_state: AdminState,
    pub description: Option<String>,
    pub mode: PortMode,
}

