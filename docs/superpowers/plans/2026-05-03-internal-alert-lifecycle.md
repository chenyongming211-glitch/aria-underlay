# Internal Alert Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an internal, auditable operation-alert lifecycle so operators can acknowledge, resolve, suppress, and expire alerts without external delivery adapters.

**Architecture:** Keep `OperationAlert` as immutable generated evidence in the existing JSONL alert sink. Add a separate lifecycle state store keyed by `dedupe_key`, a small API manager that enforces RBAC and writes product audit before changing state, then expose it through `aria-underlay-ops` commands and enriched alert listing.

**Tech Stack:** Rust, serde JSON/JSONL file stores, existing `AuthorizationPolicy`, existing `ProductAuditStore`, CLI integration tests.

---

### Task 1: Lifecycle Store And API Tests

**Files:**
- Create: `tests/alert_lifecycle_tests.rs`
- Modify: `src/telemetry/alerts.rs`
- Modify: `src/telemetry/audit.rs`
- Modify: `src/telemetry/mod.rs`
- Modify: `src/authz.rs`
- Create: `src/api/alert_lifecycle.rs`
- Modify: `src/api/mod.rs`

- [x] Write tests that prove `Operator` can acknowledge an alert, product audit is written first, lifecycle history is preserved, terminal states reject later transitions, and audit-write failure leaves lifecycle state unchanged.
- [x] Run `cargo test --test alert_lifecycle_tests` and confirm the new tests fail because the lifecycle types and manager do not exist yet. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.
- [x] Implement `OperationAlertLifecycleStatus`, lifecycle records/events, in-memory and JSON-file lifecycle stores.
- [x] Add alert lifecycle `AdminAction` values and fail-closed role rules.
- [x] Add `ProductAuditRecord::alert_lifecycle_transition` and a JSONL `JsonFileProductAuditStore`.
- [x] Add `AlertLifecycleManager` that validates input, authorizes, writes product audit, then transitions lifecycle state.
- [x] Run `cargo test --test alert_lifecycle_tests` and confirm the lifecycle tests pass. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.

### Task 2: CLI Lifecycle Commands

**Files:**
- Modify: `src/ops_cli.rs`
- Modify: `tests/ops_cli_tests.rs`

- [x] Add a CLI test that seeds an alert, runs `ack-alert`, then verifies `list-alerts` includes `lifecycle.status = Acknowledged` and product audit contains `alert.acknowledged`.
- [x] Run `cargo test --test ops_cli_tests` and confirm the new CLI test fails before implementation. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.
- [x] Add `ack-alert`, `resolve-alert`, `suppress-alert`, and `expire-alert` commands.
- [x] Require `--alert-state-path`, `--product-audit-path`, `--dedupe-key`, `--operator`, `--role`, and `--reason` for lifecycle writes.
- [x] Enrich `list-alerts` and `alert-summary` with optional `--alert-state-path` lifecycle status counts while keeping existing top-level alert fields stable.
- [x] Run `cargo test --test ops_cli_tests` and confirm the CLI tests pass. Local Rust toolchain is unavailable in this workspace; GitHub Actions is the Rust verification gate.

### Task 3: Documentation And Verification

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`
- Modify: `docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md`

- [x] Document lifecycle commands, required files, status meanings, RBAC rules, and product-audit behavior.
- [x] Update current progress and bug inventory so internal alert lifecycle is no longer listed as fully open.
- [x] Run `git diff --check`.
- [x] Run the local test commands available in this checkout.
- [ ] Commit only the lifecycle-related files, push, and wait for GitHub Actions to pass before starting the next package.
