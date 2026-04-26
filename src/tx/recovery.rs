use std::collections::BTreeSet;

use crate::model::DeviceId;
use crate::tx::journal::{TxJournalRecord, TxPhase};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub recovered: usize,
    pub in_doubt: usize,
    pub pending: usize,
    pub tx_ids: Vec<String>,
    pub decisions: Vec<RecoveryDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDecision {
    pub tx_id: String,
    pub phase: TxPhase,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    Noop,
    DiscardPreparedChanges,
    AdapterRecover,
    ManualIntervention,
}

pub fn classify_recovery(record: &TxJournalRecord) -> RecoveryDecision {
    RecoveryDecision {
        tx_id: record.tx_id.clone(),
        phase: record.phase.clone(),
        action: recovery_action_for_phase(&record.phase),
    }
}

pub fn in_doubt_records_for_devices(
    records: &[TxJournalRecord],
    device_ids: &[DeviceId],
) -> Vec<TxJournalRecord> {
    let requested = device_ids.iter().collect::<BTreeSet<_>>();
    records
        .iter()
        .filter(|record| record.phase == TxPhase::InDoubt)
        .filter(|record| record.devices.iter().any(|device_id| requested.contains(device_id)))
        .cloned()
        .collect()
}

fn recovery_action_for_phase(phase: &TxPhase) -> RecoveryAction {
    match phase {
        TxPhase::Started | TxPhase::Preparing | TxPhase::Prepared => {
            RecoveryAction::DiscardPreparedChanges
        }
        TxPhase::Committing
        | TxPhase::Verifying
        | TxPhase::FinalConfirming
        | TxPhase::RollingBack
        | TxPhase::Recovering => RecoveryAction::AdapterRecover,
        TxPhase::InDoubt => RecoveryAction::ManualIntervention,
        TxPhase::Committed | TxPhase::RolledBack | TxPhase::Failed => RecoveryAction::Noop,
    }
}
