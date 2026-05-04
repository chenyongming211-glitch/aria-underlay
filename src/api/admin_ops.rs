use std::sync::Arc;

use crate::adapter_client::mapper::AdapterOperationStatus;
use crate::adapter_client::AdapterClientPool;
use crate::authz::{AdminAction, AuthorizationPolicy, AuthorizationRequest};
use crate::api::force_resolve::{
    ForceResolveTransactionRequest, ForceResolveTransactionResponse,
};
use crate::api::force_unlock::{ForceUnlockRequest, ForceUnlockResponse};
use crate::api::recovery_ops::{in_doubt_summary_from_record, validate_force_resolve_request};
use crate::api::transactions::{
    ListInDoubtTransactionsRequest, ListInDoubtTransactionsResponse,
};
use crate::device::DeviceInventory;
use crate::proto::adapter::RequestContext;
use crate::telemetry::{EventSink, ProductAuditRecord, ProductAuditStore, UnderlayEvent};
use crate::tx::{EndpointLockTable, TxJournalStore, TxPhase};
use crate::{UnderlayError, UnderlayResult};

#[derive(Clone)]
pub(crate) struct AdminOps {
    inventory: DeviceInventory,
    journal: Arc<dyn TxJournalStore>,
    endpoint_locks: EndpointLockTable,
    event_sink: Arc<dyn EventSink>,
    authorization_policy: Arc<dyn AuthorizationPolicy>,
    product_audit_store: Arc<dyn ProductAuditStore>,
    adapter_pool: AdapterClientPool,
}

impl AdminOps {
    pub(crate) fn new(
        inventory: DeviceInventory,
        journal: Arc<dyn TxJournalStore>,
        endpoint_locks: EndpointLockTable,
        event_sink: Arc<dyn EventSink>,
        authorization_policy: Arc<dyn AuthorizationPolicy>,
        product_audit_store: Arc<dyn ProductAuditStore>,
        adapter_pool: AdapterClientPool,
    ) -> Self {
        Self {
            inventory,
            journal,
            endpoint_locks,
            event_sink,
            authorization_policy,
            product_audit_store,
            adapter_pool,
        }
    }

    pub(crate) fn list_in_doubt_transactions(
        &self,
        request: ListInDoubtTransactionsRequest,
    ) -> UnderlayResult<ListInDoubtTransactionsResponse> {
        let mut transactions: Vec<_> = self
            .journal
            .list_recoverable()?
            .into_iter()
            .filter(|record| record.phase == TxPhase::InDoubt)
            .filter(|record| {
                request
                    .device_id
                    .as_ref()
                    .map(|device_id| record.devices.contains(device_id))
                    .unwrap_or(true)
            })
            .map(in_doubt_summary_from_record)
            .collect();
        transactions.sort_by(|left, right| left.tx_id.cmp(&right.tx_id));

        Ok(ListInDoubtTransactionsResponse { transactions })
    }

    pub(crate) async fn force_unlock(
        &self,
        request: ForceUnlockRequest,
    ) -> UnderlayResult<ForceUnlockResponse> {
        let managed = self.inventory.get(&request.device_id)?;
        let mut client = self.adapter_pool.client(&managed.info.adapter_endpoint)?;
        let context = RequestContext {
            request_id: request.request_id.clone(),
            tx_id: String::new(),
            trace_id: request
                .trace_id
                .clone()
                .unwrap_or_else(|| request.request_id.clone()),
            tenant_id: managed.info.tenant_id.clone(),
            site_id: managed.info.site_id.clone(),
        };
        let outcome = client
            .force_unlock(
                &managed.info,
                &context,
                request.lock_owner,
                request.reason,
                request.break_glass_enabled,
            )
            .await?;

        Ok(ForceUnlockResponse {
            device_id: request.device_id,
            unlocked: matches!(outcome.status, AdapterOperationStatus::Committed),
            warnings: outcome.warnings,
        })
    }

    pub(crate) async fn force_resolve_transaction(
        &self,
        request: ForceResolveTransactionRequest,
    ) -> UnderlayResult<ForceResolveTransactionResponse> {
        validate_force_resolve_request(&request)?;
        let trace_id = request
            .trace_id
            .clone()
            .unwrap_or_else(|| request.request_id.clone());

        let Some(candidate) = self.journal.get(&request.tx_id)? else {
            return Err(UnderlayError::AdapterOperation {
                code: "TX_NOT_FOUND".into(),
                message: format!("transaction {} was not found", request.tx_id),
                retryable: false,
                errors: Vec::new(),
            });
        };

        let _endpoint_guard = self.endpoint_locks.acquire_many(&candidate.devices).await?;
        let Some(record) = self.journal.get(&request.tx_id)? else {
            return Err(UnderlayError::AdapterOperation {
                code: "TX_NOT_FOUND".into(),
                message: format!("transaction {} was not found", request.tx_id),
                retryable: false,
                errors: Vec::new(),
            });
        };
        if record.phase != TxPhase::InDoubt {
            return Err(UnderlayError::AdapterOperation {
                code: "TX_NOT_IN_DOUBT".into(),
                message: format!(
                    "transaction {} is {:?}, not InDoubt",
                    record.tx_id, record.phase
                ),
                retryable: false,
                errors: Vec::new(),
            });
        }

        let authorization = self.authorization_policy.authorize(&AuthorizationRequest::new(
            request.request_id.clone(),
            trace_id.clone(),
            request.operator.clone(),
            AdminAction::ForceResolveTransaction,
        ))?;
        self.product_audit_store
            .append(ProductAuditRecord::force_resolve_requested(
                request.request_id.clone(),
                trace_id.clone(),
                record.tx_id.clone(),
                authorization.operator_id,
                request.reason.clone(),
            ))
            .map_err(product_audit_error)?;

        let previous_phase = record.phase.clone();
        let resolved = record
            .clone()
            .with_manual_resolution(
                request.operator.clone(),
                request.reason.clone(),
                request.request_id.clone(),
                trace_id.clone(),
            )
            .with_phase(TxPhase::ForceResolved);
        self.journal.put(&resolved)?;
        self.event_sink.emit(UnderlayEvent::transaction_force_resolved(
            request.request_id,
            trace_id,
            record.tx_id.clone(),
            previous_phase.clone(),
            &record.devices,
            request.operator,
            request.reason,
        ));

        Ok(ForceResolveTransactionResponse {
            tx_id: record.tx_id,
            previous_phase,
            resolved_phase: TxPhase::ForceResolved,
            devices: record.devices,
            resolved: true,
            warnings: Vec::new(),
        })
    }
}

fn product_audit_error(error: UnderlayError) -> UnderlayError {
    match error {
        UnderlayError::ProductAuditWriteFailed(_) => error,
        other => UnderlayError::ProductAuditWriteFailed(other.to_string()),
    }
}
