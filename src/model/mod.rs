pub mod common;
pub mod interface;
pub mod vlan;

pub use common::{is_canonical_identifier, DeviceId, DeviceRole, Vendor};
pub use interface::{AdminState, InterfaceConfig, PortMode};
pub use vlan::VlanConfig;
