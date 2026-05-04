# Product API Routing Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a handler-facing product operations API facade with mock session extraction.

**Architecture:** Add `src/api/product_api.rs` as a small facade over `ProductOpsManager`. It owns store references and a `ProductSessionExtractor`, extracts a request-scoped session, builds a request-scoped authorization policy, and returns typed product API responses.

**Tech Stack:** Rust, serde, existing `ProductOpsManager`, existing authz and telemetry traits.

---

### Task 1: Product API Contract Tests

**Files:**
- Create: `tests/product_api_contract_tests.rs`

- [ ] **Step 1: Write failing tests**

Add contract tests for summary listing, missing identity, auditor export, operator denial, and audit-write-failure fail-closed behavior.

- [ ] **Step 2: Run focused tests to verify red**

Run:

```bash
cargo test --test product_api_contract_tests
```

Expected locally if `cargo` exists: compile failure because `api::product_api` does not exist yet. If `cargo` is unavailable, record that local Rust execution is blocked and rely on GitHub Actions after implementation.

### Task 2: Product API Facade

**Files:**
- Create: `src/api/product_api.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Add request and response envelopes**

Define `ProductApiRequest<T>` and `ProductApiResponse<T>`.

- [ ] **Step 2: Add session extractor boundary**

Define `ProductSession`, `ProductSessionExtractor`, and `HeaderProductSessionExtractor`.

- [ ] **Step 3: Add facade methods**

Implement `ProductOpsApi::list_operation_summaries` and `ProductOpsApi::export_product_audit`.

- [ ] **Step 4: Wire module export**

Add `pub mod product_api;` in `src/api/mod.rs`.

### Task 3: Docs

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`

- [ ] **Step 1: Document facade semantics**

Record that this is a handler-facing product API skeleton, not an HTTP server.

- [ ] **Step 2: Record remaining gaps**

Keep real HTTP routing, internal token/session validation, and UI as open work.

### Task 4: Verification and Release Gate

**Files:**
- All changed files

- [ ] **Step 1: Run local runnable checks**

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests -q
cargo test --test product_api_contract_tests
```

- [ ] **Step 2: Commit and push**

Stage only relevant files, commit, push to `origin main`, and wait for GitHub Actions to pass before moving to the next package.
