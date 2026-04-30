use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn is_canonical(&self) -> bool {
        is_canonical_identifier(&self.0)
    }

    pub fn canonical_rule() -> &'static str {
        "must be non-empty and contain only ASCII letters, digits, '-' or '_'"
    }
}

pub fn is_canonical_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Vendor {
    Huawei,
    H3c,
    Cisco,
    Ruijie,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceRole {
    LeafA,
    LeafB,
}
