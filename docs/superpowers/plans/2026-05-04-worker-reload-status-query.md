# Worker Reload Status Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local and product-facing read APIs for worker reload checkpoint status.

**Architecture:** Keep the daemon checkpoint as the source of truth. Add thin read-only adapters in CLI, product manager, product API, and product HTTP router. Product reads are RBAC-gated but do not write product audit records.

**Tech Stack:** Rust, serde JSON, existing product API/HTTP route skeleton, existing `WorkerReloadCheckpoint`.

---

### Task 1: Tests

**Files:**
- Modify: `tests/ops_cli_tests.rs`
- Modify: `tests/product_http_route_tests.rs`
- Modify: `tests/product_ops_rbac_tests.rs`

- [ ] Add a CLI test that writes a `WorkerReloadCheckpoint`, runs `aria-underlay-ops worker-reload-status --checkpoint-path <file>`, and asserts status/generation.
- [ ] Add a product HTTP test that a Viewer can read the checkpoint through `/product/v1/worker-reload/status:get`.
- [ ] Add a product manager test that an unassigned operator cannot read the checkpoint.

### Task 2: Core Read Helper and RBAC

**Files:**
- Modify: `src/worker/daemon.rs`
- Modify: `src/authz.rs`
- Modify: `src/api/product_ops.rs`

- [ ] Add `WorkerReloadCheckpoint::from_path`.
- [ ] Add `AdminAction::GetWorkerReloadStatus`.
- [ ] Allow all assigned roles to perform `GetWorkerReloadStatus`.
- [ ] Add `ProductGetWorkerReloadStatusRequest`.
- [ ] Add `ProductOpsManager::get_worker_reload_status`.

### Task 3: Product API, HTTP, and CLI

**Files:**
- Modify: `src/api/product_api.rs`
- Modify: `src/api/product_http.rs`
- Modify: `src/ops_cli.rs`

- [ ] Add `ProductOpsApi::get_worker_reload_status`.
- [ ] Add `WORKER_RELOAD_STATUS_GET_PATH`.
- [ ] Route `POST /product/v1/worker-reload/status:get`.
- [ ] Add CLI command `worker-reload-status --checkpoint-path <file>`.

### Task 4: Docs

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] Document local CLI and product HTTP query.
- [ ] Move checkpoint query from open operational gap to implemented local/product query surface.

### Task 5: Verification and Publish

**Files:**
- All modified files

- [ ] Run `git diff --check`.
- [ ] Run `python3 -m pytest adapter-python/tests -q`.
- [ ] Try `cargo test --test ops_cli_tests --test product_http_route_tests --test product_ops_rbac_tests`; record local cargo limitation if unavailable.
- [ ] Commit, push, and wait for GitHub Actions to pass.
