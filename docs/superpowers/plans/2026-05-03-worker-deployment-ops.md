# Worker Deployment Ops Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deployment samples and an offline worker config preflight command.

**Architecture:** Keep deployment checks in `src/worker/deployment.rs`, wire a read-only `check-worker-config` command into `src/ops_cli.rs`, and keep systemd/tmpfiles/config samples under `docs/examples`. The command returns a JSON report and fails closed on invalid config.

**Tech Stack:** Rust, serde JSON, existing worker daemon config types, existing CLI binary tests.

---

### Task 1: Deployment Preflight Tests

**Files:**
- Create: `tests/worker_deployment_tests.rs`
- Modify: `tests/ops_cli_tests.rs`

- [ ] **Step 1: Write failing tests**

Add tests for checked-in deployment samples, strict path success, invalid schedule rejection, missing strict path rejection, and CLI `check-worker-config`.

- [ ] **Step 2: Run focused tests to verify red**

Run:

```bash
cargo test --test worker_deployment_tests --test ops_cli_tests
```

Expected locally if `cargo` exists: compile failure because `worker::deployment` and `check-worker-config` do not exist yet. If `cargo` is unavailable, record that local Rust execution is blocked and rely on GitHub Actions after implementation.

### Task 2: Preflight Implementation

**Files:**
- Create: `src/worker/deployment.rs`
- Modify: `src/worker/mod.rs`

- [ ] **Step 1: Implement report and checker types**

Create `WorkerDeploymentPreflightReport`, `WorkerDeploymentPathCheck`, and `WorkerDeploymentPreflight`.

- [ ] **Step 2: Implement semantic checks**

Validate alert summary dependency, schedules, summary retention, and journal GC retention without starting workers.

- [ ] **Step 3: Implement strict filesystem checks**

Check required parent directories and roots, with temporary write probes for existing directories.

### Task 3: CLI Wiring

**Files:**
- Modify: `src/ops_cli.rs`

- [ ] **Step 1: Add command dispatch**

Add `check-worker-config` and usage text.

- [ ] **Step 2: Print report and fail closed**

Print pretty JSON in all cases and return non-zero when `report.valid=false`.

### Task 4: Deployment Samples and Docs

**Files:**
- Create: `docs/examples/underlay-worker-daemon.production.json`
- Create: `docs/examples/systemd/aria-underlay-worker.service`
- Create: `docs/examples/tmpfiles.d/aria-underlay.conf`
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`

- [ ] **Step 1: Add checked-in samples**

Add production JSON config, systemd unit, and tmpfiles.d directories.

- [ ] **Step 2: Update operator docs**

Document preflight, install paths, strict path checks, and the remaining packaging boundary.

### Task 5: Verification and Release Gate

**Files:**
- All changed files

- [ ] **Step 1: Run local runnable checks**

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests -q
cargo test --test worker_deployment_tests --test ops_cli_tests
```

- [ ] **Step 2: Commit and push**

Stage only relevant files, commit, push to `origin main`, and wait for GitHub Actions to pass before moving to the next package.
