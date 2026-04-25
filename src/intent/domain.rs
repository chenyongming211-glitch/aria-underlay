use serde::{Deserialize, Serialize};

use crate::intent::{interface::InterfaceIntent, vlan::VlanIntent};
use crate::model::{DeviceRole, Vendor};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnderlayDomainIntent {
    pub domain_id: String,
    pub topology: UnderlayTopology,
    pub endpoints: Vec<ManagementEndpointIntent>,
    pub members: Vec<SwitchMemberIntent>,
    pub vlans: Vec<VlanIntent>,
    pub interfaces: Vec<InterfaceIntent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnderlayTopology {
    StackSingleManagementIp,
    MlagDualManagementIp,
    SmallFabric,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementEndpointIntent {
    pub endpoint_id: String,
    pub host: String,
    pub port: u16,
    pub secret_ref: String,
    pub vendor_hint: Option<Vendor>,
    pub model_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchMemberIntent {
    pub member_id: String,
    pub role: Option<DeviceRole>,
    pub management_endpoint_id: String,
}
