use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclAction {
    Permit,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclProtocol {
    Ip,
    Tcp,
    Udp,
    Icmp,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AclDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclKind {
    AdvancedIpv4,
    BasicIpv4,
}

impl Default for AclKind {
    fn default() -> Self {
        Self::AdvancedIpv4
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEndpoint {
    pub address: String,
    pub wildcard: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclRule {
    pub sequence: u16,
    pub action: AclAction,
    pub protocol: AclProtocol,
    pub source: Option<AclEndpoint>,
    pub destination: Option<AclEndpoint>,
    pub source_port_eq: Option<u16>,
    pub destination_port_eq: Option<u16>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclConfig {
    pub acl_id: u16,
    #[serde(default)]
    pub kind: AclKind,
    pub name: Option<String>,
    pub description: Option<String>,
    pub rules: Vec<AclRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclBinding {
    pub interface_name: String,
    pub direction: AclDirection,
    pub acl_id: u16,
}

impl AclBinding {
    pub fn key(&self) -> String {
        acl_binding_key(&self.interface_name, &self.direction)
    }
}

pub fn acl_binding_key(interface_name: &str, direction: &AclDirection) -> String {
    let direction = match direction {
        AclDirection::Inbound => "inbound",
        AclDirection::Outbound => "outbound",
    };
    format!("{interface_name}|{direction}")
}
