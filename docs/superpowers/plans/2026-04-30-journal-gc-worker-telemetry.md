# Journal GC Worker Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit a structured telemetry event whenever journal/artifact GC completes successfully.

**Architecture:** Keep `JournalGc` as the cleanup engine. Add a small `JournalGcWorker` wrapper that runs GC once and emits `UnderlayJournalGcCompleted` through the existing `EventSink` abstraction.

**Tech Stack:** Rust, Tokio tests, existing `telemetry` event/sink types.

---

### Task 1: Add GC Telemetry Event Builder

**Files:**
- Modify: `src/telemetry/events.rs`
- Test: `tests/gc_tests.rs`

- [ ] **Step 1: Write failing event mapping test**

Add a test that calls `UnderlayEvent::journal_gc_completed("req-gc", "trace-gc", &report)` and asserts the event kind and count fields.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test journal_gc_completed_event_includes_cleanup_counts`

Expected: compile failure because `journal_gc_completed` does not exist.

- [ ] **Step 3: Implement event builder**

Add `UnderlayEvent::journal_gc_completed()` in `src/telemetry/events.rs`. It should set `kind` to `UnderlayJournalGcCompleted`, preserve request/trace IDs, and serialize report counts into `fields`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test journal_gc_completed_event_includes_cleanup_counts`

Expected: pass.

### Task 2: Add Single-Run GC Worker

**Files:**
- Modify: `src/worker/gc.rs`
- Test: `tests/gc_tests.rs`

- [ ] **Step 1: Write failing worker test**

Add a test that constructs `JournalGcWorker`, runs `run_once_and_emit()`, and checks that one event was emitted with the same cleanup counts as the report.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test gc_worker_emits_completion_event_after_successful_run`

Expected: compile failure because `JournalGcWorker` does not exist.

- [ ] **Step 3: Implement worker wrapper**

Add `JournalGcWorker` with `new(gc, policy, event_sink)`, optional `with_request_context()`, and async `run_once_and_emit()`.

- [ ] **Step 4: Run focused and full tests**

Run:

```bash
cargo test gc_worker_emits_completion_event_after_successful_run
cargo test journal_gc_completed_event_includes_cleanup_counts
cargo test
```

Expected: all pass in an environment with Rust installed.
