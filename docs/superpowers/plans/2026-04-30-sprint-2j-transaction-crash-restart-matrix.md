# Sprint 2J Transaction Crash/Restart Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove transaction recovery behavior across service recreation without requiring a real switch.

**Architecture:** Add a file-backed shadow store beside the existing file-backed journal, then add tests that recreate `AriaUnderlayService` from fresh store instances pointing at the same directories. The matrix covers pending journal recovery, shadow persistence after successful apply, and force-resolved records remaining terminal after restart.

**Tech Stack:** Rust, Tokio tests, existing fake gRPC adapter, JSON file stores.

---

### Task 1: File-Backed Shadow Store

**Files:**
- Modify: `src/state/shadow.rs`
- Modify: `src/state/mod.rs`
- Test: `tests/shadow_store_tests.rs`

- [x] Add failing tests for `JsonFileShadowStateStore` round-trip, revision increment after recreating the store, deterministic list order, remove, and path sanitization.
- [x] Implement `JsonFileShadowStateStore` with atomic temp-file write plus rename, matching `InMemoryShadowStateStore` revision semantics.
- [x] Export `JsonFileShadowStateStore` from `src/state/mod.rs`.
- [x] Run `cargo test --test shadow_store_tests`.

### Task 2: Crash/Restart Recovery Matrix

**Files:**
- Modify: `tests/recovery_tests.rs`

- [x] Add a test that writes a recoverable journal record to disk, recreates the service with a fresh journal instance, and verifies recovery marks the record `InDoubt`.
- [x] Add a test that force-resolves an `InDoubt` file-backed record, recreates the service, and verifies it no longer appears in recovery or list-in-doubt.
- [x] Run `cargo test --test recovery_tests`.

### Task 3: Successful Apply Persists Shadow Across Service Recreation

**Files:**
- Modify: `tests/transaction_gate_tests.rs`

- [x] Add a test using fake adapter + file-backed journal + file-backed shadow.
- [x] Apply an intent successfully, recreate both stores from disk, and assert the desired shadow state is still present with revision 1.
- [x] Run `cargo test --test transaction_gate_tests`.

### Task 4: Documentation and Verification

**Files:**
- Modify: `docs/progress-2026-04-26.md`

- [x] Document that crash/restart confidence now covers file-backed journal and shadow stores, still excluding real switch NETCONF semantics.
- [x] Run targeted Rust tests and `git diff --check`.
- [x] Commit the completed slice.

### 2026-05-01 Follow-up Matrix Expansion

- [x] Verify terminal `Committed`, `Failed`, `RolledBack`, and `ForceResolved`
  journal records survive store recreation but do not re-enter recovery.
- [x] Verify corrupt file-backed journal records fail recovery scans closed.
- [x] Verify file-backed journal `.tmp` crash residue is ignored after restart.
- [x] Verify corrupt file-backed shadow records fail reads closed after restart.
- [x] Verify file-backed shadow `.tmp` crash residue is ignored after restart.
