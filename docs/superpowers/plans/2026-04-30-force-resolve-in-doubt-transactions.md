# Force Resolve In-Doubt Transactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a non-device-touching operations API to manually clear unresolved `InDoubt` transactions with an auditable journal trail.

**Architecture:** Keep automatic recovery and manual resolution separate. `recover_pending_transactions()` continues to classify and recover what it can; `force_resolve_transaction()` is a break-glass operation that only accepts existing `InDoubt` records, locks affected endpoints, re-reads the journal under lock, writes a terminal manual-resolution phase, and emits an audit event.

**Tech Stack:** Rust service core, transaction journal, endpoint locks, telemetry event sink, cargo tests.

---

## Scope

Implement now:

- Rust API request/response for force resolving a transaction.
- Terminal `TxPhase::ForceResolved`.
- Journal fields for manual resolution metadata.
- Service method `force_resolve_transaction()`.
- Audit event for manual transaction resolution.
- Tests for successful force resolve, break-glass validation, non-`InDoubt` rejection, and event emission.

Do not implement now:

- Device-side state reconciliation.
- Adapter calls.
- External gRPC/protobuf endpoint.
- UI/operator workflow.

## Semantics

- Request must include `request_id`, `trace_id`, `tx_id`, `operator`, `reason`, and `break_glass_enabled=true`.
- Empty operator or reason is rejected.
- Missing transaction is rejected.
- Only current `InDoubt` records can be force resolved.
- The service locks the record's devices, re-reads the record, then updates it.
- Updated journal record:
  - `phase = ForceResolved`
  - `manual_resolution.operator = request.operator`
  - `manual_resolution.reason = request.reason`
  - `manual_resolution.request_id = request.request_id`
  - `manual_resolution.trace_id = response trace_id`
  - `manual_resolution.resolved_at_unix_secs = now`
- `ForceResolved` is terminal and no longer blocks new transactions.
- The service emits `transaction.force_resolved` telemetry with `operator`, `reason`, and device count.

## Tasks

### Task 1: API And Journal Types

**Files:**
- Create: `src/api/force_resolve.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/api/underlay_service.rs`
- Modify: `src/tx/journal.rs`
- Modify: `src/tx/recovery.rs`
- Modify: `src/worker/gc.rs`

- [x] Add `ForceResolveTransactionRequest` and `ForceResolveTransactionResponse`.
- [x] Add `TxPhase::ForceResolved`.
- [x] Add `TxManualResolution` metadata to `TxJournalRecord` with serde default.
- [x] Add helper `with_manual_resolution()`.
- [x] Treat `ForceResolved` as terminal in recovery and GC.

### Task 2: Service Implementation

**Files:**
- Modify: `src/api/service.rs`

- [x] Validate break-glass, operator, reason, and `tx_id`.
- [x] Fetch journal record and require `InDoubt`.
- [x] Lock record devices and re-read journal under lock.
- [x] Write terminal `ForceResolved` record with manual metadata.
- [x] Emit audit telemetry event.

### Task 3: Tests

**Files:**
- Modify: `tests/recovery_tests.rs`
- Modify: `tests/transaction_tests.rs`
- Modify: `tests/telemetry_tests.rs`

- [x] Verify an `InDoubt` record can be force resolved and no longer appears in `list_recoverable()`.
- [x] Verify force resolving without break-glass is rejected and leaves the record unchanged.
- [x] Verify force resolving non-`InDoubt` records is rejected.
- [x] Verify journal round-trips manual resolution metadata.
- [x] Verify telemetry/audit event maps to `transaction.force_resolved`.

### Task 4: Verification

**Commands:**

- [ ] `cargo test recovery`
- [ ] `cargo test transaction`
- [ ] `cargo test telemetry`
- [ ] `cargo test`
- [ ] `git diff --check`

Local note: this shell currently lacks `cargo`/`rustc`, so Rust verification is expected to run in GitHub Actions unless the local toolchain is installed.

Current local verification:

- `python3 -m pytest adapter-python/tests -q`: passed, 188 tests.
- `git diff --check`: passed.
- Rust local verification blocked: `cargo`, `rustc`, and `rustfmt` are not installed in this shell.
