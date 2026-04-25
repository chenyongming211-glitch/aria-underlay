use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::diff::{compute_diff, ChangeSet};
use crate::planner::device_plan::DeviceDesiredState;
use crate::state::DeviceShadowState;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunPlan {
    pub change_sets: Vec<ChangeSet>,
}

impl DryRunPlan {
    pub fn is_noop(&self) -> bool {
        self.change_sets.iter().all(ChangeSet::is_empty)
    }
}

pub fn build_dry_run_plan(
    desired_states: &[DeviceDesiredState],
    current_states: &[DeviceShadowState],
) -> UnderlayResult<DryRunPlan> {
    let current_by_device = current_states
        .iter()
        .map(|state| (state.device_id.clone(), state))
        .collect::<BTreeMap<_, _>>();

    let mut change_sets = Vec::with_capacity(desired_states.len());
    for desired in desired_states {
        let current = current_by_device
            .get(&desired.device_id)
            .ok_or_else(|| UnderlayError::InvalidDeviceState(format!(
                "missing current state for device {}",
                desired.device_id.0
            )))?;
        change_sets.push(compute_diff(desired, current));
    }

    Ok(DryRunPlan { change_sets })
}
