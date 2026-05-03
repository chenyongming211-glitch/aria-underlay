# Operator Operations Entrypoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the current local operation summary, alert, GC, and drift capabilities usable by an operator without reading source code or JSONL internals.

**Architecture:** Keep the existing JSONL stores and worker daemon traits as the local/offline operations backend. Add a formal `aria-underlay-ops` binary for read-only operator inspection, keep `transaction_ops` as an example wrapper, and document product audit/RBAC as the future backend boundary rather than mixing it into local files.

**Tech Stack:** Rust CLI binary, serde JSON output, existing `JsonFileOperationSummaryStore`, `JsonFileOperationAlertSink`, `UnderlayWorkerDaemonConfig`, Markdown runbooks, GitHub Actions CI.

---

### Task 1: Daemon Config Sample

**Files:**
- Create: `docs/examples/underlay-worker-daemon.local.json`
- Modify: `tests/worker_daemon_tests.rs`

- [x] Add a sample worker config using relative local paths under `var/aria-underlay`.
- [x] Add a test that parses the sample config with `UnderlayWorkerDaemonConfig::from_path()`.
- [x] Assert the sample enables operation summary, operation alert, journal GC, and drift audit sections.

### Task 2: Formal Ops CLI

**Files:**
- Modify: `Cargo.toml`
- Create: `src/bin/aria_underlay_ops.rs`
- Modify: `examples/transaction_ops.rs`
- Create: `tests/ops_cli_tests.rs`

- [x] Add the `aria-underlay-ops` binary.
- [x] Support existing `list-in-doubt`, `force-resolve`, `list-operations`, and `operation-summary` commands.
- [x] Add `list-alerts --operation-alert-path <file> [--severity Warning|Critical] [--limit <n>]`.
- [x] Add `alert-summary --operation-alert-path <file> [--severity Warning|Critical]`.
- [x] Keep JSON output stable for scripts.
- [x] Keep `examples/transaction_ops.rs` as a compatibility wrapper that points users at the binary.

### Task 3: Operator Runbook

**Files:**
- Create: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`

- [x] Document how to start `aria-underlay-worker` with the sample config.
- [x] Document how to inspect operation summaries, attention-required records, alerts, GC, drift, recovery, and force-resolve.
- [x] Document what each local file does and how retention/checkpointing affects output.
- [x] Record that this is the local/offline operations entrypoint, not the final product audit backend.

### Task 4: Product Audit Backend and RBAC Design

**Files:**
- Create: `docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md`
- Modify: `docs/aria-underlay-development-plan.md`

- [x] Define the production audit backend boundary behind the current operation summary traits.
- [x] Define operator roles and permissions for read-only summary, alert read, force-resolve, force-unlock, retention, and config changes.
- [x] Define fail-closed requirements when audit or authorization writes fail.
- [x] Define migration from JSONL local mode to product-backed mode.

### Task 5: Verification and Commit

**Files:**
- Verify all changed files.

- [ ] Run `git diff --check`.
- [ ] Run `python3 -m pytest adapter-python/tests -q`.
- [ ] Run Rust checks in GitHub Actions because local `cargo` is unavailable.
- [ ] Commit and push the package.
- [ ] Wait for GitHub Actions to pass before reporting completion.
