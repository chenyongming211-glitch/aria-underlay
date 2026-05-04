use std::sync::Arc;
use std::sync::Mutex;

use crate::telemetry::audit::{AuditRecord, OperationAuditStore};
use crate::telemetry::events::UnderlayEvent;
use crate::telemetry::events::UnderlayEventKind;
use crate::telemetry::ops::OperationSummaryStore;
use crate::utils::time::now_unix_secs;

pub trait EventSink: std::fmt::Debug + Send + Sync {
    fn emit(&self, event: UnderlayEvent);
}

#[derive(Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: UnderlayEvent) {}
}

#[derive(Debug, Default)]
pub struct StderrEventSink;

impl EventSink for StderrEventSink {
    fn emit(&self, event: UnderlayEvent) {
        eprintln!("{}", format_underlay_event_log_line(&event));
    }
}

#[derive(Debug, Default)]
pub struct InMemoryEventSink {
    events: Mutex<Vec<UnderlayEvent>>,
}

impl InMemoryEventSink {
    pub fn events(&self) -> Vec<UnderlayEvent> {
        self.events
            .lock()
            .expect("in-memory event sink mutex should not be poisoned")
            .clone()
    }
}

impl EventSink for InMemoryEventSink {
    fn emit(&self, event: UnderlayEvent) {
        self.events
            .lock()
            .expect("in-memory event sink mutex should not be poisoned")
            .push(event);
    }
}

#[derive(Debug, Clone)]
pub struct RecordingEventSink {
    inner: Arc<dyn EventSink>,
    operation_summaries: Arc<dyn OperationSummaryStore>,
    operation_audit: Option<Arc<dyn OperationAuditStore>>,
}

impl RecordingEventSink {
    pub fn new(
        inner: Arc<dyn EventSink>,
        operation_summaries: Arc<dyn OperationSummaryStore>,
    ) -> Self {
        Self {
            inner,
            operation_summaries,
            operation_audit: None,
        }
    }

    pub fn with_operation_audit_store(
        mut self,
        operation_audit: Arc<dyn OperationAuditStore>,
    ) -> Self {
        self.operation_audit = Some(operation_audit);
        self
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: UnderlayEvent) {
        if let Err(err) = self.operation_summaries.record_event(&event) {
            if event.kind != UnderlayEventKind::UnderlayAuditWriteFailed {
                let audit = AuditRecord::from_event(&event);
                self.inner.emit(UnderlayEvent::audit_write_failed(
                    event.request_id.clone(),
                    event.trace_id.clone(),
                    audit.action,
                    format!("{err}"),
                ));
            }
        }
        if let Some(operation_audit) = &self.operation_audit {
            if let Err(err) = operation_audit.record_event(&event) {
                if event.kind != UnderlayEventKind::UnderlayAuditWriteFailed {
                    let audit = AuditRecord::from_event(&event);
                    self.inner.emit(UnderlayEvent::operation_audit_write_failed(
                        event.request_id.clone(),
                        event.trace_id.clone(),
                        audit.action,
                        format!("{err}"),
                    ));
                }
            }
        }
        self.inner.emit(event);
    }
}

pub fn format_underlay_event_log_line(event: &UnderlayEvent) -> String {
    let audit = AuditRecord::from_event(event);
    let mut fields = vec![
        ("ts".to_string(), now_unix_secs().to_string()),
        ("level".to_string(), event_log_level(event).to_string()),
        ("action".to_string(), audit.action),
        ("result".to_string(), audit.result),
        ("request_id".to_string(), audit.request_id),
        ("trace_id".to_string(), audit.trace_id),
    ];
    if let Some(tx_id) = audit.tx_id {
        fields.push(("tx_id".to_string(), tx_id));
    }
    if let Some(device_id) = audit.device_id {
        fields.push(("device_id".to_string(), device_id.0));
    }
    if let Some(phase) = &event.phase {
        fields.push(("phase".to_string(), format!("{phase:?}")));
    }
    if let Some(strategy) = &event.strategy {
        fields.push(("strategy".to_string(), format!("{strategy:?}")));
    }
    if let Some(error_code) = audit.error_code {
        fields.push(("error_code".to_string(), error_code));
    }
    if let Some(error_message) = audit.error_message {
        fields.push(("error_message".to_string(), error_message));
    }
    for (key, value) in &event.fields {
        fields.push((format!("field.{key}"), value.clone()));
    }

    fields
        .into_iter()
        .map(|(key, value)| format!("{key}={}", format_log_value(&value)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn event_log_level(event: &UnderlayEvent) -> &'static str {
    if event.error_code.is_some()
        || matches!(
            &event.kind,
            UnderlayEventKind::UnderlayAuditWriteFailed
                | UnderlayEventKind::UnderlayTransactionFailed
                | UnderlayEventKind::UnderlayTransactionInDoubt
        )
    {
        "error"
    } else if matches!(
        &event.kind,
        UnderlayEventKind::UnderlayDriftDetected
            | UnderlayEventKind::UnderlayDeviceLockTimeout
            | UnderlayEventKind::UnderlayForceUnlockRequested
    ) {
        "warn"
    } else {
        "info"
    }
}

fn format_log_value(value: &str) -> String {
    if value.chars().all(is_unquoted_log_char) {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"<unprintable>\"".into())
    }
}

fn is_unquoted_log_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | ',' | '@')
}
