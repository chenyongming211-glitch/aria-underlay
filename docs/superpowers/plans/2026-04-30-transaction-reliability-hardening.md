# Transaction Reliability Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the highest-risk transaction reliability gaps that do not require real switches.

**Architecture:** Keep the existing journal, endpoint lock, shadow store, and NETCONF adapter boundaries. Fix behavior with fail-closed status handling, safer journal finalization, recovery revalidation under lock, and adapter diagnostics that preserve original failure context.

**Tech Stack:** Rust service core, Tokio, in-memory/file journal stores, Python NETCONF adapter, pytest, cargo test.

---

## Scope

Confirmed defects to fix now:

- `src/api/service.rs`: terminal `Committed` journal is written before shadow state persistence.
- `src/api/service.rs`: mixed device success and failure aggregates to `SuccessWithWarning`.
- `src/api/service.rs`: `recover_pending_transactions()` classifies records before endpoint locks are held.
- `src/tx/journal.rs`: journal error fields overwrite previous failure context.
- `adapter-python/aria_underlay_adapter/backends/netconf.py`: discard failure can hide the original prepare error.
- `adapter-python/aria_underlay_adapter/backends/netconf.py`: auth classification uses an overly broad `"auth"` substring.
- `adapter-python/aria_underlay_adapter/backends/mock_netconf.py`: unknown port mode normalizes to access.

Confirmed non-defect or deferred items:

- `src/tx/endpoint_lock.rs` already uses `tokio::sync::Mutex`, not `std::sync::Mutex`.
- Empty public intents are already rejected by intent validation, but internal empty desired-state handling should still fail closed.
- Renderer/parser `production_ready=False` is intentional fail-closed behavior until real-device evidence exists.
- gRPC connection pooling is a reliability optimization, not an ACID protocol fix for this sprint.
- Durable production wiring for `JsonFileTxJournalStore` remains a later deployment/config task.

## Tasks

### Task 1: Rust Transaction Status And Shadow Finalization

**Files:**
- Modify: `src/api/service.rs`
- Test: `src/api/service.rs`

- [x] Add unit coverage for aggregate status:
  - empty device result list returns `Failed`.
  - success plus failed returns `Failed`.
  - success plus in-doubt returns `InDoubt`.
  - all success with warning still returns `SuccessWithWarning`.
- [x] Change `aggregate_apply_status()` so partial failure is never reported as `SuccessWithWarning`.
- [x] In the successful single-endpoint apply path, write shadow state before terminal `Committed`.
- [x] If shadow write fails after adapter success, write the journal as `InDoubt` with the shadow error and return `ApplyStatus::InDoubt`.

### Task 2: Recovery Revalidation Under Endpoint Lock

**Files:**
- Modify: `src/api/service.rs`
- Test: `tests/recovery_tests.rs`

- [x] Add a test showing a recoverable candidate that becomes `Committed` before locked recovery is not recovered again.
- [ ] Add a test showing empty-device recoverable records are marked `InDoubt` instead of looping silently.
- [x] Refactor recovery so each candidate is locked by device before the journal record is re-read and classified.
- [x] Mark `Recovering` only after the locked, reloaded record is still recoverable.

### Task 3: Journal Error History

**Files:**
- Modify: `src/tx/journal.rs`
- Test: `tests/transaction_tests.rs`

- [x] Add `TxJournalErrorEvent` with `phase`, `code`, `message`, and `created_at_unix_secs`.
- [x] Add `error_history: Vec<TxJournalErrorEvent>` to `TxJournalRecord` with serde default for backward compatibility.
- [x] Make `with_error()` keep current `error_code/error_message` and append an error event.
- [x] Add round-trip tests proving multiple errors are preserved.

### Task 4: Python NETCONF Adapter Fail-Closed Behavior

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf.py`
- Modify: `adapter-python/aria_underlay_adapter/backends/mock_netconf.py`
- Test: `adapter-python/tests/test_netconf_backend.py`
- Test: `adapter-python/tests/test_mock_netconf_backend.py`

- [x] Add pytest coverage proving prepare errors are preserved when discard also fails.
- [x] Add pytest coverage proving authorization text is not classified as authentication failure.
- [x] Add pytest coverage proving unknown mock port mode fails verification instead of becoming access.
- [x] Preserve the original prepare error and append discard failure details to the raw summary.
- [x] Replace broad auth substring matching with exact class names and bounded authentication phrases.
- [x] Make mock mode normalization fail closed for unknown kinds.

### Task 5: Verification

**Files:**
- Existing tests only.

- [ ] Run targeted Rust tests:
  - `cargo test aggregate`
  - `cargo test recovery`
  - `cargo test transaction`
- [x] Run targeted Python tests:
  - `python3 -m pytest adapter-python/tests/test_netconf_backend.py adapter-python/tests/test_mock_netconf_backend.py -q`
- [ ] Run full available suites:
  - `cargo test`
  - `python3 -m pytest adapter-python/tests -q`
- [ ] Run formatting/diff checks:
  - `cargo fmt`
  - `git diff --check`

Current local verification note:

- `python3 -m pytest adapter-python/tests/test_netconf_backend.py adapter-python/tests/test_mock_netconf_backend.py -q`: passed, 74 tests.
- `python3 -m pytest adapter-python/tests -q`: passed, 188 tests.
- `git diff --check`: passed.
- Rust verification is blocked locally because `cargo` and `rustc` are not installed in this shell.
