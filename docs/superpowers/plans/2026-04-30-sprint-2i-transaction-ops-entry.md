# Sprint 2I Transaction Ops Entry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make unresolved `InDoubt` transactions discoverable and manually resolvable through an auditable, offline ops entry that does not require real switches.

**Architecture:** Add a read-only `list_in_doubt_transactions` service API backed by the transaction journal, then expose both list and `force_resolve_transaction` through a small example command that operates on a JSON journal directory. Keep the existing force-resolve safety gates: only `InDoubt` can be resolved, `operator` and `reason` are required, and `break_glass_enabled` must be explicit.

**Tech Stack:** Rust 2021, serde/serde_json, existing `TxJournalStore`, existing `AriaUnderlayService`, existing integration tests.

---

### Task 1: List In-Doubt Transactions API

**Files:**
- Create: `src/api/transactions.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/api/underlay_service.rs`
- Modify: `src/api/service.rs`
- Test: `tests/recovery_tests.rs`

- [x] **Step 1: Write the failing test**

Add a service test that stores one `InDoubt`, one `Prepared`, and one `ForceResolved` record, calls `list_in_doubt_transactions`, and expects only the `InDoubt` record with its error history.

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test list_in_doubt_transactions_returns_only_in_doubt_records`

Expected if Rust is installed: FAIL because the API does not exist yet.

Local note: this shell returned `zsh:1: command not found: cargo`, so Rust RED/GREEN verification is deferred to GitHub Actions.

- [x] **Step 3: Write minimal implementation**

Add request/response structs, export the module, add the trait method, and implement the method by filtering `journal.list_recoverable()` for `TxPhase::InDoubt`.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test list_in_doubt_transactions_returns_only_in_doubt_records`

Expected if Rust is installed: PASS.

Local note: this shell returned `zsh:1: command not found: cargo`, so Rust pass/fail is verified by CI.

### Task 2: Journal-Directory Ops Example

**Files:**
- Create: `examples/transaction_ops.rs`
- Modify: `docs/progress-2026-04-26.md`
- Test: GitHub Actions Rust example compilation

- [x] **Step 1: Add the example command**

Create `transaction_ops` with two commands:

```text
cargo run --example transaction_ops -- list-in-doubt --journal-root /path/to/journal
cargo run --example transaction_ops -- force-resolve --journal-root /path/to/journal --tx-id tx-123 --operator alice --reason "verified out of band" --break-glass
```

- [x] **Step 2: Preserve fail-closed behavior**

The example must call service methods instead of editing journal JSON directly, so the existing validation and audit metadata remain the only mutation path.

- [x] **Step 3: Update progress docs**

Document that Sprint 2I provides a non-device-touching ops entry for listing and resolving `InDoubt` transactions, but does not add a production RPC server or UI.

### Task 3: Verification

**Files:**
- Existing test and docs files from Tasks 1-2

- [x] **Step 1: Local checks**

Run:

```bash
python3 -m pytest adapter-python/tests -q
git diff --check
```

Result: `python3 -m pytest adapter-python/tests -q` passed with `188 passed`; `git diff --check` exited cleanly.

- [ ] **Step 2: Rust checks**

Run locally if available:

```bash
cargo test list_in_doubt_transactions_returns_only_in_doubt_records
cargo test recovery
cargo check --examples
```

If local Rust is unavailable, push and use GitHub Actions as the Rust verification gate.

- [ ] **Step 3: Commit and push**

Commit message:

```text
feat: add transaction ops entry
```

Watch GitHub Actions until the pushed commit is green.
