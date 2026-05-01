use std::sync::Arc;
use std::sync::Mutex;

use crate::telemetry::events::UnderlayEvent;
use crate::telemetry::ops::OperationSummaryStore;

pub trait EventSink: std::fmt::Debug + Send + Sync {
    fn emit(&self, event: UnderlayEvent);
}

#[derive(Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: UnderlayEvent) {}
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
}

impl RecordingEventSink {
    pub fn new(
        inner: Arc<dyn EventSink>,
        operation_summaries: Arc<dyn OperationSummaryStore>,
    ) -> Self {
        Self {
            inner,
            operation_summaries,
        }
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: UnderlayEvent) {
        let _ = self.operation_summaries.record_event(&event);
        self.inner.emit(event);
    }
}
