pub mod common;
pub mod interface;
pub mod vlan;

pub use common::{DeviceId, DeviceRole, Vendor};
pub use interface::{AdminState, InterfaceConfig, PortMode};
pub use vlan::VlanConfig;

