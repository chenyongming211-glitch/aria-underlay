use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::telemetry::{
    JsonFileOperationAlertLifecycleStore, JsonFileOperationAlertSink, OperationAlert,
    OperationAlertLifecycleRecord, OperationAlertLifecycleStatus, OperationAlertLifecycleStore,
    OperationAlertSeverity,
};
use crate::UnderlayResult;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOperationAlertsRequest {
    pub operation_alert_path: PathBuf,
    #[serde(default)]
    pub alert_state_path: Option<PathBuf>,
    #[serde(default)]
    pub severity: Option<OperationAlertSeverity>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOperationAlertsResponse {
    pub alerts: Vec<OperationAlertView>,
    pub overview: OperationAlertOverview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAlertView {
    #[serde(flatten)]
    pub alert: OperationAlert,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<OperationAlertLifecycleRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAlertOverview {
    pub matched_alerts: usize,
    pub returned_alerts: usize,
    pub critical: usize,
    pub warning: usize,
    pub open: usize,
    pub acknowledged: usize,
    pub resolved: usize,
    pub suppressed: usize,
    pub expired: usize,
    pub by_action: BTreeMap<String, usize>,
    pub by_result: BTreeMap<String, usize>,
    pub by_device: BTreeMap<String, usize>,
}

pub fn list_operation_alerts(
    request: ListOperationAlertsRequest,
) -> UnderlayResult<ListOperationAlertsResponse> {
    let alerts = filtered_alerts(&request)?;
    let lifecycle_records = lifecycle_records(&request)?;
    let returned_alerts = limit_alerts(&alerts, &lifecycle_records, request.limit);
    let overview =
        OperationAlertOverview::from_alerts(&alerts, returned_alerts.len(), &lifecycle_records);
    Ok(ListOperationAlertsResponse {
        alerts: returned_alerts,
        overview,
    })
}

impl OperationAlertOverview {
    pub fn from_alerts(
        alerts: &[OperationAlert],
        returned_alerts: usize,
        lifecycle_records: &BTreeMap<String, OperationAlertLifecycleRecord>,
    ) -> Self {
        let mut overview = Self {
            matched_alerts: alerts.len(),
            returned_alerts,
            ..Default::default()
        };

        for alert in alerts {
            match alert.severity {
                OperationAlertSeverity::Critical => overview.critical += 1,
                OperationAlertSeverity::Warning => overview.warning += 1,
            }
            increment(&mut overview.by_action, &alert.action);
            increment(&mut overview.by_result, &alert.result);
            if let Some(device_id) = &alert.device_id {
                increment(&mut overview.by_device, &device_id.0);
            }
            overview.record_lifecycle_status(
                lifecycle_records
                    .get(&alert.dedupe_key)
                    .map(|record| &record.status)
                    .unwrap_or(&OperationAlertLifecycleStatus::Open),
            );
        }

        overview
    }

    fn record_lifecycle_status(&mut self, status: &OperationAlertLifecycleStatus) {
        match status {
            OperationAlertLifecycleStatus::Open => self.open += 1,
            OperationAlertLifecycleStatus::Acknowledged => self.acknowledged += 1,
            OperationAlertLifecycleStatus::Resolved => self.resolved += 1,
            OperationAlertLifecycleStatus::Suppressed => self.suppressed += 1,
            OperationAlertLifecycleStatus::Expired => self.expired += 1,
        }
    }
}

fn filtered_alerts(request: &ListOperationAlertsRequest) -> UnderlayResult<Vec<OperationAlert>> {
    Ok(JsonFileOperationAlertSink::new(&request.operation_alert_path)
        .list()?
        .into_iter()
        .filter(|alert| {
            request
                .severity
                .as_ref()
                .map(|severity| alert.severity == *severity)
                .unwrap_or(true)
        })
        .collect())
}

fn lifecycle_records(
    request: &ListOperationAlertsRequest,
) -> UnderlayResult<BTreeMap<String, OperationAlertLifecycleRecord>> {
    let Some(alert_state_path) = &request.alert_state_path else {
        return Ok(BTreeMap::new());
    };
    Ok(JsonFileOperationAlertLifecycleStore::new(alert_state_path)
        .list()?
        .into_iter()
        .map(|record| (record.dedupe_key.clone(), record))
        .collect())
}

fn limit_alerts(
    alerts: &[OperationAlert],
    lifecycle_records: &BTreeMap<String, OperationAlertLifecycleRecord>,
    limit: Option<usize>,
) -> Vec<OperationAlertView> {
    alerts
        .iter()
        .take(limit.unwrap_or(alerts.len()))
        .map(|alert| OperationAlertView {
            alert: alert.clone(),
            lifecycle: lifecycle_records.get(&alert.dedupe_key).cloned(),
        })
        .collect()
}

fn increment(map: &mut BTreeMap<String, usize>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}
