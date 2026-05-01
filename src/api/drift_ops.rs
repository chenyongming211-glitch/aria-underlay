use crate::state::drift::DriftPolicy;
use crate::{UnderlayError, UnderlayResult};

pub(super) fn drift_policy_error(
    policy: DriftPolicy,
    device_list: &str,
) -> UnderlayResult<()> {
    match policy {
        DriftPolicy::BlockNewTransaction => Err(UnderlayError::AdapterOperation {
            code: "DRIFT_BLOCKED".into(),
            message: format!("device has unresolved out-of-band drift: {device_list}"),
            retryable: false,
            errors: Vec::new(),
        }),
        DriftPolicy::AutoReconcile => Err(UnderlayError::AdapterOperation {
            code: "DRIFT_AUTORECONCILE_UNIMPLEMENTED".into(),
            message: format!(
                "auto reconcile is not implemented for drifted device(s): {device_list}"
            ),
            retryable: false,
            errors: Vec::new(),
        }),
        DriftPolicy::ReportOnly => Ok(()),
    }
}
