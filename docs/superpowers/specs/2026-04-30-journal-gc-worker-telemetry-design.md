# Journal GC Worker Telemetry Design

## Goal

Add a small worker-facing entrypoint for journal/artifact GC that emits a structured telemetry event after each successful GC run.

## Scope

This change implements a single-run worker wrapper, not a periodic background loop. The loop can be added later once runtime ownership and shutdown behavior are decided.

## Design

`JournalGc` remains responsible for filesystem cleanup and retention behavior. A new `JournalGcWorker` composes `JournalGc`, `RetentionPolicy`, request/trace identifiers, and an `EventSink`.

`JournalGcWorker::run_once_and_emit()` runs GC once, emits `UnderlayJournalGcCompleted` on success, and returns the existing `JournalGcReport`. If GC fails, the error is returned and no success event is emitted.

The telemetry event stores the cleanup counts in `fields`:

- `journals_deleted`
- `journals_retained`
- `artifacts_deleted`

## Testing

Add Rust tests covering:

- successful worker run emits one `UnderlayJournalGcCompleted` event with GC counts
- event builder maps `JournalGcReport` fields into telemetry fields
