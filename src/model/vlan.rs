use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VlanConfig {
    pub vlan_id: u16,
    pub name: Option<String>,
    pub description: Option<String>,
}

