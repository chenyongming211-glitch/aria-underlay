pub mod domain;
pub mod interface;
pub mod switch_pair;
pub mod validation;
pub mod vlan;

pub use domain::{
    ManagementEndpointIntent, SwitchMemberIntent, UnderlayDomainIntent, UnderlayTopology,
};
pub use switch_pair::{SwitchIntent, SwitchPairIntent};
