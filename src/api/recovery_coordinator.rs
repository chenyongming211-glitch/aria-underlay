use std::sync::Arc;

use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::adapter_client::{tx_request_context, AdapterClientPool};
use crate::api::apply::journal_error_fields;
use crate::api::recovery_ops::{
    change_set_for_record, desired_state_for_record, error_summary,
    final_confirm_recovery_in_doubt_error, merge_recovery_phase,
    recover_phase_from_adapter_status,
};
use crate::device::DeviceInventory;
use crate::tx::recovery::{classify_recovery, RecoveryAction, RecoveryReport};
use crate::tx::{
    EndpointLockTable, TransactionStrategy, TxContext, TxJournalRecord, TxJournalStore, TxPhase,
};
use crate::UnderlayResult;

#[derive(Clone)]
pub(crate) struct RecoveryCoordinator {
    inventory: DeviceInventory,
    journal: Arc<dyn TxJournalStore>,
    endpoint_locks: EndpointLockTable,
    adapter_pool: AdapterClientPool,
}

impl RecoveryCoordinator {
    pub(crate) fn new(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
        endpoint_locks: EndpointLockTable,
        adapter_pool: AdapterClientPool,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks,
            adapter_pool,
        }
    }

    pub(crate) async fn recover_pending_transactions(&self) -> UnderlayResult<RecoveryReport> {
        let recoverable = self.journal.list_recoverable()?;
        let mut decisions = Vec::with_capacity(recoverable.len());
        let mut recovered = 0;

        for candidate in recoverable {
            let _endpoint_guard = self.endpoint_locks.acquire_many(&candidate.devices).await?;
            let Some(record) = self.journal.get(&candidate.tx_id)? else {
                continue;
            };
            let decision = classify_recovery(&record);
            decisions.push(decision.clone());
            if !record.phase.requires_recovery() {
                continue;
            }

            match decision.action {
                RecoveryAction::Noop => {}
                RecoveryAction::ManualIntervention => {
                    if record.phase != TxPhase::InDoubt {
                        self.journal
                            .put(&record.clone().with_phase(TxPhase::InDoubt))?;
                    }
                }
                RecoveryAction::DiscardPreparedChanges | RecoveryAction::AdapterRecover => {
                    self.journal
                        .put(&record.clone().with_phase(TxPhase::Recovering))?;

                    match self.recover_record(&record, decision.action).await {
                        Ok(phase) => {
                            let terminal =
                                matches!(phase, TxPhase::Committed | TxPhase::RolledBack);
                            self.journal.put(&record.clone().with_phase(phase))?;
                            if terminal {
                                recovered += 1;
                            }
                        }
                        Err(err) => {
                            let (code, message) = journal_error_fields(&err);
                            self.journal.put(
                                &record
                                    .clone()
                                    .with_phase(TxPhase::InDoubt)
                                    .with_error(code, message),
                            )?;
                        }
                    }
                }
            }
        }

        let pending_records = self.journal.list_recoverable()?;
        let in_doubt = pending_records
            .iter()
            .filter(|record| record.phase == TxPhase::InDoubt)
            .count();
        let tx_ids = pending_records
            .iter()
            .map(|record| record.tx_id.clone())
            .collect::<Vec<_>>();

        Ok(RecoveryReport {
            recovered,
            in_doubt,
            pending: pending_records.len(),
            tx_ids,
            decisions,
        })
    }

    async fn recover_record(
        &self,
        record: &TxJournalRecord,
        action: RecoveryAction,
    ) -> UnderlayResult<TxPhase> {
        if record.phase == TxPhase::FinalConfirming
            && record.strategy == Some(TransactionStrategy::ConfirmedCommit)
        {
            return self.recover_final_confirming_record(record).await;
        }

        if record.devices.is_empty() {
            return Ok(match action {
                RecoveryAction::DiscardPreparedChanges => TxPhase::RolledBack,
                _ => TxPhase::InDoubt,
            });
        }

        let tx_context = TxContext {
            tx_id: record.tx_id.clone(),
            request_id: record.request_id.clone(),
            trace_id: record.trace_id.clone(),
        };
        let mut merged_phase = None;

        for device_id in &record.devices {
            let managed = self.inventory.get(device_id)?;
            let mut client = self.adapter_pool.client(&managed.info.adapter_endpoint)?;
            let rpc_context = tx_request_context(&managed.info, &tx_context);
            let outcome = client
                .recover_with_context(&managed.info, &rpc_context, record.strategy, action)
                .await?;
            let phase = recover_phase_from_adapter_status(action, outcome.status);
            merged_phase = Some(merge_recovery_phase(merged_phase, phase));
        }

        Ok(merged_phase.unwrap_or(TxPhase::InDoubt))
    }

    async fn recover_final_confirming_record(
        &self,
        record: &TxJournalRecord,
    ) -> UnderlayResult<TxPhase> {
        if record.devices.is_empty() {
            return Ok(TxPhase::InDoubt);
        }

        let tx_context = TxContext {
            tx_id: record.tx_id.clone(),
            request_id: record.request_id.clone(),
            trace_id: record.trace_id.clone(),
        };
        let mut merged_phase = None;

        for device_id in &record.devices {
            let managed = self.inventory.get(device_id)?;
            let mut client = self.adapter_pool.client(&managed.info.adapter_endpoint)?;
            let rpc_context = tx_request_context(&managed.info, &tx_context);

            let final_confirm_summary = match client
                .final_confirm_with_context(&managed.info, &rpc_context)
                .await
            {
                Ok(outcome) if outcome.status == AdapterOperationStatus::Committed => {
                    merged_phase = Some(merge_recovery_phase(
                        merged_phase,
                        TxPhase::Committed,
                    ));
                    continue;
                }
                Ok(outcome) => {
                    format!("final confirm returned status {:?}", outcome.status)
                }
                Err(err) => error_summary("final confirm", &err),
            };

            let verify_summary =
                if let (Some(desired), Some(change_set)) = (
                    desired_state_for_record(record, device_id),
                    change_set_for_record(record, device_id),
                ) {
                    match client
                        .verify_with_context_for_change_set(
                            &managed.info,
                            &rpc_context,
                            desired,
                            change_set,
                        )
                        .await
                    {
                        Ok(verify)
                            if matches!(
                                verify.status,
                                AdapterOperationStatus::NoChange
                                    | AdapterOperationStatus::Committed
                            ) =>
                        {
                            merged_phase = Some(merge_recovery_phase(
                                merged_phase,
                                TxPhase::Committed,
                            ));
                            continue;
                        }
                        Ok(verify) => {
                            format!("verify returned status {:?}", verify.status)
                        }
                        Err(err) => error_summary("verify", &err),
                    }
                } else {
                    format!(
                        "journal record {} has no desired state or change set for device {}",
                        record.tx_id, device_id.0
                    )
                };

            match client
                .recover_with_context(
                    &managed.info,
                    &rpc_context,
                    record.strategy,
                    RecoveryAction::AdapterRecover,
                )
                .await
            {
                Ok(outcome) => {
                    let phase =
                        recover_phase_from_adapter_status(RecoveryAction::AdapterRecover, outcome.status);
                    match phase {
                        TxPhase::Committed | TxPhase::RolledBack => {
                            merged_phase = Some(merge_recovery_phase(merged_phase, phase));
                        }
                        _ => {
                            return Err(final_confirm_recovery_in_doubt_error(
                                record,
                                device_id,
                                final_confirm_summary,
                                verify_summary,
                                format!("adapter recover returned status {:?}", outcome.status),
                            ));
                        }
                    }
                }
                Err(err) => {
                    return Err(final_confirm_recovery_in_doubt_error(
                        record,
                        device_id,
                        final_confirm_summary,
                        verify_summary,
                        error_summary("adapter recover", &err),
                    ));
                }
            }
        }

        Ok(merged_phase.unwrap_or(TxPhase::InDoubt))
    }
}
