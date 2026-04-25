use crate::engine::diff::ChangeSet;

#[derive(Debug, Clone)]
pub struct DryRunPlan {
    pub change_sets: Vec<ChangeSet>,
}

