# Confirmed Commit Recovery Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the `FinalConfirming` crash window where a confirmed commit can be applied on the device but recovery repeatedly tries only `cancel-commit`.

**Architecture:** Persist recovery-safe desired device state and change-set scope in the transaction journal, then handle `FinalConfirming + ConfirmedCommit` as a roll-forward-first recovery path. Recovery first retries `final_confirm`; if that cannot prove success, it verifies the persisted desired state using the original touched-resource scope; only when success cannot be proven does it attempt the existing adapter rollback path or mark the transaction `InDoubt`.

**Tech Stack:** Rust transaction service, JSON transaction journal, tonic adapter client, existing fake adapter test server.

---

### Task 1: Refresh Bug Documentation

**Files:**
- Modify: `docs/bug-inventory-2026-04-30.md`

- [x] **Step 1: Mark current vs superseded findings**

Add a status matrix that separates currently confirmed P0/P1/P2 findings from stale 2026-04-26 claims already fixed by later commits.

### Task 2: Add Regression Coverage

**Files:**
- Modify: `tests/recovery_tests.rs`

- [ ] **Step 1: Write the failing test**

Add a recovery test that seeds a journal record in `TxPhase::FinalConfirming`, with `TransactionStrategy::ConfirmedCommit`, persisted desired state, and persisted change set. The fake adapter should fail `FinalConfirm`, fail generic `Recover`, but succeed `Verify`. Expected recovery result: terminal `Committed`.

- [ ] **Step 2: Run the focused Rust test**

Run: `cargo test --test recovery_tests recover_pending_transactions_confirms_final_confirming_by_verifying_desired_state`

Expected before implementation: fail because current recovery never uses final-confirm retry or desired-state verify for `FinalConfirming`.

### Task 3: Persist Recovery Desired State and Change Scope

**Files:**
- Modify: `src/tx/journal.rs`
- Modify: `src/api/service.rs`
- Modify: `tests/gc_tests.rs`

- [ ] **Step 1: Add `desired_states` and `change_sets` to `TxJournalRecord`**

Add `#[serde(default)] pub desired_states: Vec<DeviceDesiredState>`, `#[serde(default)] pub change_sets: Vec<ChangeSet>`, and matching builder helpers. This keeps old journal files readable.

- [ ] **Step 2: Store desired state at transaction start**

When creating a transaction record in `apply_single_endpoint_state`, call `with_desired_states(vec![desired.clone()])` and `with_change_sets(plan.change_sets.clone())` before the first journal write.

### Task 4: Fix `FinalConfirming` Recovery

**Files:**
- Modify: `src/api/service.rs`

- [ ] **Step 1: Add a dedicated `recover_final_confirming_record()` path**

When `record.phase == FinalConfirming` and strategy is `ConfirmedCommit`, recover per device by:

1. retrying `final_confirm_with_context`;
2. if final-confirm cannot prove success, verifying the persisted desired state with the persisted change-set scope;
3. if verification cannot prove success, falling back to existing adapter recover;
4. returning an explicit `FINAL_CONFIRM_RECOVERY_IN_DOUBT` error if no path can prove `Committed` or `RolledBack`.

- [ ] **Step 2: Keep other phases unchanged**

`Committing` and `Verifying` continue to use the existing adapter recover path, so recovery does not blindly roll forward before verification.

### Task 5: Verify and Record Result

**Files:**
- Modify: `docs/bug-inventory-2026-04-30.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Run local verification**

Run Python tests and any available Rust checks. If local `cargo` is unavailable, rely on GitHub Actions for Rust and state that explicitly.

- [ ] **Step 2: Update docs**

Mark the P0 finding fixed with the commit SHA and describe the remaining P1/P2 backlog.
