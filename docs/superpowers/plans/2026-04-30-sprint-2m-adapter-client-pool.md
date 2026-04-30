# Adapter Client Pool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reuse gRPC adapter channels by endpoint across onboarding, state refresh, apply, recovery, drift audit, and force unlock.

**Architecture:** Add `AdapterClientPool` beside `AdapterClient`. The pool caches tonic `Channel` values keyed by endpoint and returns a fresh `AdapterClient` facade per call. Services own a pool and request clients through it instead of calling `AdapterClient::connect` directly.

**Tech Stack:** Rust 2021, tonic `Channel`, dashmap, tokio tests, GitHub Actions for Rust verification.

---

### Task 1: Pool Core

**Files:**
- Modify: `src/adapter_client/client.rs`
- Modify: `src/adapter_client/mod.rs`
- Test: `tests/adapter_client_pool_tests.rs`

- [ ] Write tests for same-endpoint reuse, different-endpoint separation, invalid endpoint rejection, and invalidate.
- [ ] Add `AdapterClient::from_channel(Channel)`.
- [ ] Add `AdapterClientPool` with `client(endpoint)`, `invalidate(endpoint)`, `cached_endpoint_count()`, and `contains_endpoint(endpoint)`.
- [ ] Export `AdapterClientPool`.

### Task 2: Service Integration

**Files:**
- Modify: `src/api/service.rs`
- Modify: `src/device/onboarding.rs`
- Modify: `src/device/bootstrap.rs`
- Test: existing Rust tests and CI integration matrix.

- [ ] Add `adapter_pool` to `AriaUnderlayService`.
- [ ] Initialize the pool in every service constructor.
- [ ] Replace direct `AdapterClient::connect` calls in service methods with `self.adapter_pool.client(...)`.
- [ ] Add pool-aware constructors for onboarding and site initialization.
- [ ] Keep current public constructors backward-compatible.

### Task 3: Documentation and Verification

**Files:**
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-2026-04-26.md`

- [ ] Mark the connection churn bug fixed.
- [ ] Record Sprint 2M behavior and limits.
- [ ] Run `git diff --check`.
- [ ] Run Python adapter tests to confirm unrelated layer remains green.
- [ ] Push and verify GitHub Actions.
