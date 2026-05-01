use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::api::force_resolve::ForceResolveTransactionRequest;
use crate::api::transactions::InDoubtTransactionSummary;
use crate::engine::diff::ChangeSet;
use crate::model::DeviceId;
use crate::planner::device_plan::DeviceDesiredState;
use crate::tx::{RecoveryAction, TxJournalRecord, TxPhase};
use crate::{UnderlayError, UnderlayResult};

use super::apply::journal_error_fields;

pub(super) fn recover_phase_from_adapter_status(
    action: RecoveryAction,
    status: AdapterOperationStatus,
) -> TxPhase {
    match (action, status) {
        (_, AdapterOperationStatus::RolledBack) => TxPhase::RolledBack,
        (RecoveryAction::AdapterRecover, AdapterOperationStatus::Committed) => TxPhase::Committed,
        (RecoveryAction::DiscardPreparedChanges, AdapterOperationStatus::NoChange) => {
            TxPhase::RolledBack
        }
        _ => TxPhase::InDoubt,
    }
}

pub(super) fn merge_recovery_phase(current: Option<TxPhase>, next: TxPhase) -> TxPhase {
    match (current, next) {
        (None, phase) => phase,
        (Some(left), right) if left == right => left,
        (Some(TxPhase::InDoubt), _) | (_, TxPhase::InDoubt) => TxPhase::InDoubt,
        _ => TxPhase::InDoubt,
    }
}

pub(super) fn in_doubt_summary_from_record(
    record: TxJournalRecord,
) -> InDoubtTransactionSummary {
    InDoubtTransactionSummary {
        tx_id: record.tx_id,
        request_id: record.request_id,
        trace_id: record.trace_id,
        phase: record.phase,
        devices: record.devices,
        strategy: record.strategy,
        error_code: record.error_code,
        error_message: record.error_message,
        error_history: record.error_history,
        created_at_unix_secs: record.created_at_unix_secs,
        updated_at_unix_secs: record.updated_at_unix_secs,
    }
}

pub(super) fn desired_state_for_record<'a>(
    record: &'a TxJournalRecord,
    device_id: &DeviceId,
) -> Option<&'a DeviceDesiredState> {
    record
        .desired_states
        .iter()
        .find(|desired| &desired.device_id == device_id)
}

pub(super) fn change_set_for_record<'a>(
    record: &'a TxJournalRecord,
    device_id: &DeviceId,
) -> Option<&'a ChangeSet> {
    record
        .change_sets
        .iter()
        .find(|change_set| &change_set.device_id == device_id)
}

pub(super) fn error_summary(operation: &str, error: &UnderlayError) -> String {
    let (code, message) = journal_error_fields(error);
    format!("{operation} failed with {code}: {message}")
}

pub(super) fn final_confirm_recovery_in_doubt_error(
    record: &TxJournalRecord,
    device_id: &DeviceId,
    final_confirm_summary: String,
    verify_summary: String,
    recover_summary: String,
) -> UnderlayError {
    UnderlayError::AdapterOperation {
        code: "FINAL_CONFIRM_RECOVERY_IN_DOUBT".into(),
        message: format!(
            "could not prove final-confirming transaction {} for device {}: {}; {}; {}",
            record.tx_id, device_id.0, final_confirm_summary, verify_summary, recover_summary
        ),
        retryable: true,
        errors: Vec::new(),
    }
}

pub(super) fn validate_force_resolve_request(
    request: &ForceResolveTransactionRequest,
) -> UnderlayResult<()> {
    if request.tx_id.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "force resolve requires tx_id".into(),
        ));
    }
    if request.operator.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "force resolve requires operator".into(),
        ));
    }
    if request.reason.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "force resolve requires reason".into(),
        ));
    }
    if !request.break_glass_enabled {
        return Err(UnderlayError::AdapterOperation {
            code: "FORCE_RESOLVE_BREAK_GLASS_REQUIRED".into(),
            message: "break-glass must be enabled to force resolve an in-doubt transaction"
                .into(),
            retryable: false,
            errors: Vec::new(),
        });
    }

    Ok(())
}
