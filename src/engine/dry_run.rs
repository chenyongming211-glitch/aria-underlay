use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::device::model_profile::DeviceModelProfile;
use crate::engine::change_plan::{build_change_plan_with_profile, ChangePlan};
use crate::engine::diff::{compute_diff, ChangeSet};
use crate::model::DeviceId;
use crate::planner::device_plan::DeviceDesiredState;
use crate::state::DeviceShadowState;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunPlan {
    pub change_sets: Vec<ChangeSet>,
    pub change_plans: Vec<ChangePlan>,
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
    build_dry_run_plan_with_profiles(desired_states, current_states, &BTreeMap::new())
}

pub fn build_dry_run_plan_with_profiles(
    desired_states: &[DeviceDesiredState],
    current_states: &[DeviceShadowState],
    profiles: &BTreeMap<DeviceId, DeviceModelProfile>,
) -> UnderlayResult<DryRunPlan> {
    let current_by_device = current_states
        .iter()
        .map(|state| (state.device_id.clone(), state))
        .collect::<BTreeMap<_, _>>();

    let mut change_sets = Vec::with_capacity(desired_states.len());
    let mut change_plans = Vec::with_capacity(desired_states.len());
    for desired in desired_states {
        let current = current_by_device
            .get(&desired.device_id)
            .ok_or_else(|| {
                UnderlayError::InvalidDeviceState(format!(
                    "missing current state for device {}",
                    desired.device_id.0
                ))
            })?;
        let change_set = compute_diff(desired, current);
        let profile = profiles.get(&desired.device_id);
        let change_plan = build_change_plan_with_profile(&change_set, profile);
        change_sets.push(change_set);
        change_plans.push(change_plan);
    }

    Ok(DryRunPlan {
        change_sets,
        change_plans,
    })
}
