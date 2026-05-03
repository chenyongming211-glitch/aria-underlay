# Worker Config Admin Ops Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add audited RBAC-protected local CLI operations for daemon retention and schedule config changes.

**Architecture:** Add a `WorkerConfigAdminManager` under `src/api` that validates, authorizes, writes product audit, and atomically updates `UnderlayWorkerDaemonConfig`. Expose it through `aria-underlay-ops` commands while keeping running daemon reload out of scope.

**Tech Stack:** Rust, serde JSON config, existing `AuthorizationPolicy`, existing `ProductAuditStore`, local CLI integration tests.

---

### Task 1: API Manager And Unit Tests

**Files:**
- Create: `tests/worker_config_admin_tests.rs`
- Create: `src/api/worker_config_admin.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/telemetry/audit.rs`
- Modify: `src/worker/daemon.rs`
- Modify: `src/telemetry/ops.rs`
- Modify: `src/worker/gc.rs`

- [x] Write tests for admin summary-retention update, viewer denial, audit write failure blocking config mutation, and invalid schedule rejection.
- [x] Run `cargo test --test worker_config_admin_tests` and confirm the new tests fail before implementation. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.
- [x] Implement config load/write helpers on `UnderlayWorkerDaemonConfig` using atomic write.
- [x] Make operation summary and journal GC retention validation callable by the manager.
- [x] Add product audit constructor for worker config admin requests.
- [x] Implement `WorkerConfigAdminManager` request/response types and mutation methods.
- [x] Run `cargo test --test worker_config_admin_tests` and confirm tests pass on an environment with Rust. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.

### Task 2: CLI Commands

**Files:**
- Modify: `src/ops_cli.rs`
- Modify: `tests/ops_cli_tests.rs`

- [x] Add a CLI test for `set-worker-schedule` that verifies config mutation and product audit.
- [x] Run `cargo test --test ops_cli_tests` and confirm the new test fails before implementation. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.
- [x] Add `set-summary-retention`, `set-gc-retention`, and `set-worker-schedule` commands.
- [x] Parse role, reason, request IDs, schedule target, retention values, and boolean `--run-immediately`.
- [x] Print JSON responses with changed target and config path.
- [x] Run `cargo test --test ops_cli_tests`. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.

### Task 3: Documentation And Verification

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`
- Modify: `docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md`

- [x] Document the new commands, RBAC rules, audit behavior, and no-hot-reload boundary.
- [x] Update progress and current bug inventory to remove retention/schedule RBAC from the fully open list.
- [x] Run `git diff --check`.
- [x] Run local runnable tests.
- [ ] Commit only worker-config-admin related files, push, and wait for GitHub Actions to pass.
