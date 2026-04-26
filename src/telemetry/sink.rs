use std::sync::Mutex;

use crate::telemetry::events::UnderlayEvent;

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
