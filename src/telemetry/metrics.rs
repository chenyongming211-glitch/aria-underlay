use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::response::ApplyStatus;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MetricName {
    TransactionTotal,
    TransactionFailedTotal,
    TransactionRollbackTotal,
    TransactionInDoubtTotal,
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
