use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::api::response::{ApplyStatus, DeviceApplyResult};
use crate::engine::dry_run::DryRunPlan;
use crate::model::DeviceId;
use crate::tx::{TransactionStrategy, TxPhase};
use crate::{AdapterErrorDetail, UnderlayError};

pub(super) fn device_results_from_plan(plan: &DryRunPlan) -> Vec<DeviceApplyResult> {
    plan.change_sets
        .iter()
        .map(|change_set| DeviceApplyResult {
            device_id: change_set.device_id.clone(),
            changed: !change_set.is_empty(),
            status: if change_set.is_empty() {
                ApplyStatus::NoOpSuccess
            } else {
                ApplyStatus::Success
            },
            tx_id: None,
            strategy: None,
            error_code: None,
            error_message: None,
            warnings: Vec::new(),
        })
        .collect()
}

pub(super) fn aggregate_apply_status(device_results: &[DeviceApplyResult]) -> ApplyStatus {
    if device_results.is_empty() {
        ApplyStatus::Failed
    } else if device_results
        .iter()
        .all(|result| result.status == ApplyStatus::NoOpSuccess)
    {
        ApplyStatus::NoOpSuccess
    } else if device_results
        .iter()
        .any(|result| result.status == ApplyStatus::InDoubt)
    {
        ApplyStatus::InDoubt
    } else if device_results.iter().all(|result| {
        matches!(
            result.status,
            ApplyStatus::Success | ApplyStatus::SuccessWithWarning | ApplyStatus::NoOpSuccess
        )
    }) {
        if device_results
            .iter()
            .any(|result| result.status == ApplyStatus::SuccessWithWarning)
        {
            ApplyStatus::SuccessWithWarning
        } else {
            ApplyStatus::Success
        }
    } else if device_results
        .iter()
        .all(|result| result.status == ApplyStatus::RolledBack)
    {
        ApplyStatus::RolledBack
    } else if device_results.len() == 1 {
        device_results[0].status.clone()
    } else {
        ApplyStatus::Failed
    }
}

pub(super) fn device_error_result(
    device_id: &DeviceId,
    changed: bool,
    tx_id: Option<String>,
    strategy: Option<TransactionStrategy>,
    error: UnderlayError,
) -> DeviceApplyResult {
    let (code, message) = journal_error_fields(&error);
    DeviceApplyResult {
        device_id: device_id.clone(),
        changed,
        status: if matches!(code.as_str(), "TX_IN_DOUBT" | "TX_REQUIRES_RECOVERY") {
            ApplyStatus::InDoubt
        } else {
            ApplyStatus::Failed
        },
        tx_id,
        strategy,
        error_code: Some(code),
        error_message: Some(message),
        warnings: Vec::new(),
    }
}

pub(super) fn commit_status_matches_strategy(
    status: AdapterOperationStatus,
    strategy: TransactionStrategy,
) -> bool {
    match strategy {
        TransactionStrategy::ConfirmedCommit => {
            status == AdapterOperationStatus::ConfirmedCommitPending
        }
        _ => status == AdapterOperationStatus::Committed,
    }
}

pub(super) fn failed_apply_phase(current: &TxPhase) -> TxPhase {
    match current {
        TxPhase::RolledBack | TxPhase::InDoubt => current.clone(),
        _ => TxPhase::Failed,
    }
}

pub(super) fn apply_status_for_failed_phase(phase: &TxPhase) -> ApplyStatus {
    match phase {
        TxPhase::RolledBack => ApplyStatus::RolledBack,
        TxPhase::InDoubt => ApplyStatus::InDoubt,
        _ => ApplyStatus::Failed,
    }
}

pub(super) fn degraded_strategy_warnings(strategy: TransactionStrategy) -> Vec<String> {
    if strategy.is_degraded() {
        vec![format!(
            "degraded transaction strategy {:?}; atomicity is weaker than confirmed commit",
            strategy
        )]
    } else {
        Vec::new()
    }
}

pub(super) fn journal_error_fields(error: &UnderlayError) -> (String, String) {
    match error {
        UnderlayError::AdapterOperation {
            code,
            message,
            errors,
            ..
        } => (code.clone(), adapter_error_message(message, errors)),
        UnderlayError::AdapterTransport(message) => ("ADAPTER_TRANSPORT".into(), message.clone()),
        UnderlayError::InvalidIntent(message) => ("INVALID_INTENT".into(), message.clone()),
        UnderlayError::AuthorizationDenied(message) => {
            ("AUTHORIZATION_DENIED".into(), message.clone())
        }
        UnderlayError::AuthenticationFailed(message) => {
            ("AUTHENTICATION_FAILED".into(), message.clone())
        }
        UnderlayError::ProductAuditWriteFailed(message) => {
            ("PRODUCT_AUDIT_WRITE_FAILED".into(), message.clone())
        }
        UnderlayError::InvalidDeviceState(message) => {
            ("INVALID_DEVICE_STATE".into(), message.clone())
        }
        UnderlayError::UnsupportedTransactionStrategy => (
            "UNSUPPORTED_TRANSACTION_STRATEGY".into(),
            "unsupported transaction strategy".into(),
        ),
        UnderlayError::DeviceAlreadyExists(device_id) => {
            ("DEVICE_ALREADY_EXISTS".into(), device_id.clone())
        }
        UnderlayError::DeviceNotFound(device_id) => ("DEVICE_NOT_FOUND".into(), device_id.clone()),
        UnderlayError::Internal(message) => ("INTERNAL".into(), message.clone()),
    }
}

fn adapter_error_message(message: &str, errors: &[AdapterErrorDetail]) -> String {
    if errors.is_empty() {
        return message.to_string();
    }

    let additional = errors
        .iter()
        .take(8)
        .map(|error| format!("{}: {}", error.code, error.message))
        .collect::<Vec<_>>()
        .join("; ");
    let truncated = errors.len().saturating_sub(8);
    if truncated == 0 {
        format!("{message}; additional adapter errors: {additional}")
    } else {
        format!("{message}; additional adapter errors: {additional}; {truncated} more")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(status: ApplyStatus) -> DeviceApplyResult {
        DeviceApplyResult {
            device_id: DeviceId("leaf-a".into()),
            changed: status != ApplyStatus::NoOpSuccess,
            status,
            tx_id: None,
            strategy: None,
            error_code: None,
            error_message: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn aggregate_empty_results_are_failed() {
        let status = aggregate_apply_status(&[]);

        assert_eq!(status, ApplyStatus::Failed);
    }

    #[test]
    fn aggregate_partial_failure_is_failed() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::Success),
            result(ApplyStatus::Failed),
        ]);

        assert_eq!(status, ApplyStatus::Failed);
    }

    #[test]
    fn aggregate_partial_rollback_is_failed() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::Success),
            result(ApplyStatus::RolledBack),
        ]);

        assert_eq!(status, ApplyStatus::Failed);
    }

    #[test]
    fn aggregate_success_with_warning_is_successful_but_warns() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::SuccessWithWarning),
            result(ApplyStatus::Success),
        ]);

        assert_eq!(status, ApplyStatus::SuccessWithWarning);
    }

    #[test]
    fn aggregate_all_degraded_successes_do_not_become_failed() {
        let status = aggregate_apply_status(&[
            result(ApplyStatus::SuccessWithWarning),
            result(ApplyStatus::SuccessWithWarning),
        ]);

        assert_eq!(status, ApplyStatus::SuccessWithWarning);
    }

    #[test]
    fn degraded_strategy_warning_only_for_degraded_strategies() {
        assert!(degraded_strategy_warnings(TransactionStrategy::ConfirmedCommit).is_empty());
        assert_eq!(
            degraded_strategy_warnings(TransactionStrategy::RunningRollbackOnError).len(),
            1
        );
    }

    #[test]
    fn journal_error_fields_preserves_additional_adapter_errors() {
        let error = UnderlayError::AdapterOperation {
            code: "FIRST".into(),
            message: "first error".into(),
            retryable: false,
            errors: vec![
                AdapterErrorDetail {
                    code: "SECOND".into(),
                    message: "second error".into(),
                },
                AdapterErrorDetail {
                    code: "THIRD".into(),
                    message: "third error".into(),
                },
            ],
        };

        let (code, message) = journal_error_fields(&error);

        assert_eq!(code, "FIRST");
        assert!(message.contains("first error"));
        assert!(message.contains("SECOND: second error"));
        assert!(message.contains("THIRD: third error"));
    }
}
