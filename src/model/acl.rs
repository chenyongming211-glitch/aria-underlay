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
    pub name: Option<String>,
    pub description: Option<String>,
    pub rules: Vec<AclRule>,
}

