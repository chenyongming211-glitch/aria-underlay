use serde::{Deserialize, Serialize};

use crate::model::AclRule;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclIntent {
    pub acl_id: u16,
    pub name: Option<String>,
    pub description: Option<String>,
    pub rules: Vec<AclRule>,
}

