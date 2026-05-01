# Operation Alert Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development for code changes and superpowers:verification-before-completion before claiming completion.

**Goal:** Deliver `attention_required` operation summaries to an operator-facing alert sink without requiring real switches or an external alerting product.

**Architecture:** Keep alert delivery downstream of `OperationSummaryStore`. `OperationSummaryStore` remains the source of operator-relevant events. A new alert worker reads attention-required summaries, converts them into deterministic `OperationAlert` records, skips previously delivered dedupe keys through a checkpoint store, delivers only new alerts to a sink, and records the checkpoint only after successful sink delivery.

**Scope:**

- Add `OperationAlert` and severity classification in telemetry.
- Add `OperationAlertSink` and `OperationAlertCheckpointStore` traits.
- Add in-memory implementations for tests.
- Add JSONL alert sink and JSON checkpoint store for local deployments.
- Add periodic worker/runtime/daemon config wiring.
- Do not add product RBAC, external webhook clients, or UI in this package.

---

### Task 1: Alert Primitives

**Files:**

- Create: `src/telemetry/alerts.rs`
- Modify: `src/telemetry/mod.rs`

- [x] **Step 1: Define alert record**

Convert each attention-required `OperationSummary` into an `OperationAlert` with:

- deterministic `dedupe_key`
- `Warning` or `Critical` severity
- request/trace/tx/device identity
- action/result/fields

- [x] **Step 2: Add sink and checkpoint traits**

Keep delivery target and dedupe persistence separate so future product alert backends can replace the JSONL sink without changing worker logic.

### Task 2: Alert Worker

**Files:**

- Create: `src/worker/operation_alerts.rs`
- Modify: `src/worker/mod.rs`
- Modify: `src/worker/runtime.rs`

- [x] **Step 1: Write failing worker tests**

Add tests for first delivery, second-run dedupe, and runtime scheduling.

Local note: `cargo test operation_alert_worker_delivers_only_new_attention_required_summaries` cannot run in this workspace because `cargo` is unavailable.

- [x] **Step 2: Implement worker**

Read `list_attention_required()`, filter delivered dedupe keys, deliver new alerts, then checkpoint delivered keys.

### Task 3: Daemon Wiring

**Files:**

- Modify: `src/worker/daemon.rs`
- Test: `tests/worker_daemon_tests.rs`

- [x] **Step 1: Add daemon config**

Add `operation_alert` config with JSONL alert path, checkpoint path, and schedule.

- [x] **Step 2: Wire JSONL sink/checkpoint**

Daemon config requires `operation_summary.path`; alert delivery without an operation summary store fails closed.

- [x] **Step 3: Verify restart dedupe**

Run the daemon twice against the same operation summary JSONL and checkpoint; alert JSONL should not duplicate already delivered alerts.

### Task 4: Verification

- [x] **Step 1: Local Python and diff checks**

Run:

```bash
python3 -m pytest adapter-python/tests -q
git diff --check
```

Result: Python adapter tests passed with `238 passed`; `git diff --check` exited cleanly.

- [x] **Step 2: Local Rust check attempt**

Run:

```bash
cargo test operation_alert_worker_delivers_only_new_attention_required_summaries
```

Result: `zsh:1: command not found: cargo`; Rust verification is deferred to GitHub Actions.

- [ ] **Step 3: Commit, push, and watch CI**

Push this package and wait for GitHub Actions to pass before starting another package.
