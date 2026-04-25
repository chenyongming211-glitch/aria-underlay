use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminState {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortMode {
    Access {
        vlan_id: u16,
    },
    Trunk {
        native_vlan: Option<u16>,
        allowed_vlans: Vec<u16>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceConfig {
    pub name: String,
    pub admin_state: AdminState,
    pub description: Option<String>,
    pub mode: PortMode,
}

