use crate::model::{InterfaceConfig, PortMode};

pub trait Normalize {
    fn normalize(self) -> Self;
}

impl Normalize for InterfaceConfig {
    fn normalize(mut self) -> Self {
        if self.description.as_deref() == Some("") {
            self.description = None;
        }

        if let PortMode::Trunk { allowed_vlans, .. } = &mut self.mode {
            allowed_vlans.sort_unstable();
            allowed_vlans.dedup();
        }

        self
    }
}

