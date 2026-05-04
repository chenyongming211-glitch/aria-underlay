# Worker Daemon Hot Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `aria-underlay-worker` adopt audited worker config changes without a process restart.

**Architecture:** Add a daemon supervisor that polls the worker config file, validates changed config before touching the current runtime, restarts the runtime only after successful validation, and writes an atomic reload checkpoint for operator visibility.

**Tech Stack:** Rust, Tokio, serde JSON, existing `UnderlayWorkerDaemonConfig`, existing `UnderlayWorkerRuntime`, existing atomic file helper.

---

### Task 1: Reload Contract Tests

**Files:**
- Modify: `tests/worker_daemon_tests.rs`

- [ ] Write a daemon test that starts a reload-enabled config with long intervals, edits the config schedule, waits until the reload checkpoint reaches generation 2, then asserts the checkpoint status is `applied`.
- [ ] Write a daemon test that changes the config to an invalid zero interval, waits until the checkpoint status is `rejected`, asserts generation is unchanged, restores valid config, and shuts down cleanly.
- [ ] Write a deployment preflight test that rejects reload enabled with `poll_interval_secs=0` or missing `checkpoint_path`.

### Task 2: Config, Checkpoint, and Validation

**Files:**
- Modify: `src/worker/daemon.rs`
- Modify: `src/worker/deployment.rs`
- Modify: `src/worker/mod.rs` if a new module is needed

- [ ] Add `reload: Option<WorkerReloadDaemonConfig>` to `UnderlayWorkerDaemonConfig`.
- [ ] Add `WorkerReloadDaemonConfig` with `enabled`, `poll_interval_secs`, and `checkpoint_path`.
- [ ] Add `WorkerReloadCheckpoint` and `WorkerReloadStatus`.
- [ ] Validate reload config in daemon construction and deployment preflight.
- [ ] Use `atomic_write` for checkpoint persistence.

### Task 3: Runtime Supervisor

**Files:**
- Modify: `src/worker/daemon.rs`
- Modify: `src/bin/aria_underlay_worker.rs`

- [ ] Add `UnderlayWorkerDaemon::run_config_path_until_shutdown(path, shutdown)`.
- [ ] If reload is disabled, keep the existing one-shot runtime path.
- [ ] If reload is enabled, start a supervised runtime task, poll the config file, and apply valid changed configs by shutting down and replacing the runtime.
- [ ] Reject invalid changed configs without stopping the current runtime.
- [ ] Update `aria-underlay-worker` to call the config-path entrypoint.

### Task 4: Docs and Samples

**Files:**
- Modify: `docs/examples/underlay-worker-daemon.local.json`
- Modify: `docs/examples/underlay-worker-daemon.production.json`
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] Add reload sections to checked-in worker daemon samples.
- [ ] Update the operator runbook to explain checkpoint states and invalid reload behavior.
- [ ] Remove online daemon hot reload from the open gap list and record remaining limits.

### Task 5: Verification and Publish

**Files:**
- All modified files

- [ ] Run `git diff --check`.
- [ ] Run `python3 -m pytest adapter-python/tests -q`.
- [ ] Try `cargo test --test worker_daemon_tests`; if local cargo is unavailable, record the limitation and rely on GitHub Actions for Rust.
- [ ] Commit, push, and wait for GitHub Actions to pass before moving to another package.
