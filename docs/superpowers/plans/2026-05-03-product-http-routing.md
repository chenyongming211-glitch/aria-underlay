# Product HTTP Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a framework-neutral product HTTP route contract on top of `ProductOpsApi`.

**Architecture:** Create `src/api/product_http.rs` with typed HTTP request/response structs and a `ProductHttpRouter` that dispatches two POST routes into `ProductOpsApi`. The router maps headers into product API metadata, serializes typed responses, and returns stable JSON errors for invalid input, RBAC denial, unknown paths, wrong methods, and audit/storage failures.

**Tech Stack:** Rust, serde/serde_json, existing `ProductOpsApi`, existing operation summary and product audit stores.

---

### Task 1: Product HTTP Route Contract Tests

**Files:**
- Create: `tests/product_http_route_tests.rs`

- [ ] **Step 1: Write the route tests**

Add tests covering success, RBAC denial, missing request ID, unknown path, wrong method, and invalid JSON against `ProductHttpRouter`.

- [ ] **Step 2: Run the focused test**

Run: `cargo test --test product_http_route_tests`

Expected locally: unavailable in this workspace because `cargo` is not installed. Expected in GitHub Actions before implementation: compile failure because `aria_underlay::api::product_http` does not exist.

### Task 2: Product HTTP Router

**Files:**
- Create: `src/api/product_http.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Implement request/response structs**

Define `ProductHttpMethod`, `ProductHttpRequest`, `ProductHttpResponse`, and `ProductHttpErrorResponse`.

- [ ] **Step 2: Implement dispatch**

Dispatch:

- `POST /product/v1/operations/summaries:query`
- `POST /product/v1/product-audit:export`

Reject unknown paths with `404` and known paths with non-`POST` methods with `405` and `allow: POST`.

- [ ] **Step 3: Implement metadata extraction**

Read `x-aria-request-id`, optional `x-aria-trace-id`, and pass all headers through to `ProductOpsApi` for session extraction.

- [ ] **Step 4: Implement error mapping**

Map `UnderlayError::InvalidIntent` to `400`, `AuthorizationDenied` to `403`, and product audit/internal errors to `500`.

### Task 3: Docs

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`

- [ ] **Step 1: Document route contract**

Record paths, headers, status semantics, and the no-listener/no-real-IdP boundary.

- [ ] **Step 2: Update current progress and open debt**

Move product API routing from fully open to framework-neutral route contract complete; leave real server, real IdP, DB backend, and UI as open product packaging work.

### Task 4: Verification and CI

**Files:**
- All package files above.

- [ ] **Step 1: Run local checks**

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests -q
cargo test --test product_http_route_tests
```

Expected: diff and Python pass locally; cargo is unavailable locally.

- [ ] **Step 2: Commit and push**

Commit message: `feat: add product http routing contract`

- [ ] **Step 3: Wait for GitHub Actions**

Wait for the CI run for the pushed commit. If it fails, inspect logs, fix, and repeat.
