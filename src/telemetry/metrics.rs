use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::response::ApplyStatus;
use crate::telemetry::events::{UnderlayEvent, UnderlayEventKind};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MetricName {
    TransactionTotal,
    TransactionFailedTotal,
    TransactionRollbackTotal,
    TransactionInDoubtTotal,
    OperationForceResolveTotal,
    OperationRecoveryTotal,
    OperationRecoveryInDoubtTotal,
    OperationDriftAuditTotal,
    OperationDriftDetectedTotal,
    OperationJournalGcTotal,
    OperationJournalGcDeletedTotal,
    OperationAuditWriteFailedTotal,
    NetconfRpcLatencyMs,
    DeviceSessionReconnectTotal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricSample {
    pub name: MetricName,
    pub value: f64,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
pub struct Metrics {
    counters: BTreeMap<MetricName, u64>,
}

impl Metrics {
    pub fn record_transaction_status(&mut self, status: &ApplyStatus) {
        self.increment(MetricName::TransactionTotal);
        match status {
            ApplyStatus::Failed => self.increment(MetricName::TransactionFailedTotal),
            ApplyStatus::RolledBack => self.increment(MetricName::TransactionRollbackTotal),
            ApplyStatus::InDoubt => self.increment(MetricName::TransactionInDoubtTotal),
            ApplyStatus::NoOpSuccess | ApplyStatus::Success | ApplyStatus::SuccessWithWarning => {}
        }
    }

    pub fn record_event(&mut self, event: &UnderlayEvent) {
        match &event.kind {
            UnderlayEventKind::UnderlayTransactionForceResolved => {
                self.increment(MetricName::OperationForceResolveTotal);
            }
            UnderlayEventKind::UnderlayRecoveryCompleted => {
                self.increment(MetricName::OperationRecoveryTotal);
                if event
                    .fields
                    .get("in_doubt")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or_default()
                    > 0
                {
                    self.increment(MetricName::OperationRecoveryInDoubtTotal);
                }
            }
            UnderlayEventKind::UnderlayDriftAuditCompleted => {
                self.increment(MetricName::OperationDriftAuditTotal);
                if event.result.as_deref() == Some("drift_detected") {
                    self.increment(MetricName::OperationDriftDetectedTotal);
                }
            }
            UnderlayEventKind::UnderlayJournalGcCompleted => {
                self.increment(MetricName::OperationJournalGcTotal);
                let deleted = event
                    .fields
                    .get("journals_deleted")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or_default()
                    + event
                        .fields
                        .get("artifacts_deleted")
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or_default();
                if deleted > 0 {
                    self.increment(MetricName::OperationJournalGcDeletedTotal);
                }
            }
            UnderlayEventKind::UnderlayAuditWriteFailed => {
                self.increment(MetricName::OperationAuditWriteFailedTotal);
            }
            _ => {}
        }
    }

    pub fn increment(&mut self, name: MetricName) {
        *self.counters.entry(name).or_insert(0) += 1;
    }

    pub fn samples(&self) -> Vec<MetricSample> {
        self.counters
            .iter()
            .map(|(name, value)| MetricSample {
                name: name.clone(),
                value: *value as f64,
                labels: BTreeMap::new(),
            })
            .collect()
    }
}
