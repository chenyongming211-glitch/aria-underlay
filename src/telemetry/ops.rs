use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::telemetry::audit::AuditRecord;
use crate::telemetry::events::{UnderlayEvent, UnderlayEventKind};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummary {
    pub request_id: String,
    pub trace_id: String,
    pub action: String,
    pub result: String,
    pub tx_id: Option<String>,
    pub device_id: Option<DeviceId>,
    pub attention_required: bool,
    pub fields: BTreeMap<String, String>,
}

impl OperationSummary {
    pub fn from_event(event: &UnderlayEvent) -> Option<Self> {
        if !is_operator_event(&event.kind) {
            return None;
        }

        let audit = AuditRecord::from_event(event);
        Some(Self {
            request_id: audit.request_id,
            trace_id: audit.trace_id,
            action: audit.action,
            result: audit.result,
            tx_id: audit.tx_id,
            device_id: audit.device_id,
            attention_required: attention_required(event),
            fields: event.fields.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub struct InMemoryOperationSummaryStore {
    summaries: Mutex<Vec<OperationSummary>>,
}

impl InMemoryOperationSummaryStore {
    pub fn record_event(&self, event: &UnderlayEvent) -> UnderlayResult<()> {
        let Some(summary) = OperationSummary::from_event(event) else {
            return Ok(());
        };
        self.summaries
            .lock()
            .map_err(|_| UnderlayError::Internal("operation summary mutex poisoned".into()))?
            .push(summary);
        Ok(())
    }

    pub fn list(&self) -> UnderlayResult<Vec<OperationSummary>> {
        Ok(self
            .summaries
            .lock()
            .map_err(|_| UnderlayError::Internal("operation summary mutex poisoned".into()))?
            .clone())
    }

    pub fn list_attention_required(&self) -> UnderlayResult<Vec<OperationSummary>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|summary| summary.attention_required)
            .collect())
    }
}

fn is_operator_event(kind: &UnderlayEventKind) -> bool {
    matches!(
        kind,
        UnderlayEventKind::UnderlayDriftDetected
            | UnderlayEventKind::UnderlayDriftAuditCompleted
            | UnderlayEventKind::UnderlayJournalGcCompleted
            | UnderlayEventKind::UnderlayRecoveryCompleted
            | UnderlayEventKind::UnderlayTransactionForceResolved
            | UnderlayEventKind::UnderlayTransactionInDoubt
    )
}

fn attention_required(event: &UnderlayEvent) -> bool {
    match &event.kind {
        UnderlayEventKind::UnderlayDriftDetected => true,
        UnderlayEventKind::UnderlayDriftAuditCompleted => {
            event.result.as_deref() == Some("drift_detected")
        }
        UnderlayEventKind::UnderlayRecoveryCompleted => {
            field_value(event, "in_doubt") > 0 || field_value(event, "pending") > 0
        }
        UnderlayEventKind::UnderlayTransactionInDoubt => true,
        _ => false,
    }
}

fn field_value(event: &UnderlayEvent, name: &str) -> u64 {
    event
        .fields
        .get(name)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
}
