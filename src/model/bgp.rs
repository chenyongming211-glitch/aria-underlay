use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpProcess {
    pub vrf: String,
    pub local_as: u32,
    pub router_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpNeighbor {
    pub vrf: String,
    pub address: String,
    pub remote_as: u32,
    pub description: Option<String>,
    pub import_policy: Option<String>,
    pub export_policy: Option<String>,
}

impl BgpNeighbor {
    pub fn key(&self) -> String {
        bgp_neighbor_key(&self.vrf, &self.address)
    }
}

pub fn bgp_neighbor_key(vrf: &str, address: &str) -> String {
    format!("{vrf}|{address}")
}
