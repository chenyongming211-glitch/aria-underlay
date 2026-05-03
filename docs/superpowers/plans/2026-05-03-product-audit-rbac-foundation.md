# Product Audit RBAC Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the first no-real-switch product audit and RBAC foundation for privileged operations.

**Architecture:** Keep local JSONL operation summaries as the local operations surface, and add product audit/RBAC as separate injectable service dependencies. The first enforcement point is `force_resolve_transaction`: authorization and product-audit prewrite must succeed before the journal is mutated. External alert delivery is explicitly out of scope; alerts remain internal records.

**Tech Stack:** Rust traits, in-memory test stores, existing `AriaUnderlayService`, existing transaction journal, existing operation summary/event pipeline, GitHub Actions CI.

---

### Task 1: Record Alert Product Direction

**Files:**
- Modify: `docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/runbooks/operator-operations.md`

- [x] Remove external webhook, enterprise IM, PagerDuty, and email delivery from the current roadmap.
- [x] Record that alerts stay internal and are queried through CLI/product API.
- [x] Record internal alert lifecycle as the follow-up direction.

### Task 2: Write RBAC and Audit Failing Tests

**Files:**
- Create: `tests/product_audit_rbac_tests.rs`

- [x] Add a test that an authorized `BreakGlassOperator` can force-resolve an `InDoubt` transaction and records a product audit prewrite.
- [x] Add a test that a `Viewer` cannot force-resolve and the journal remains `InDoubt`.
- [x] Add a test that a product audit write failure blocks force-resolve before the journal changes.
- [x] Add a test for the role matrix: `BreakGlassOperator` and `Admin` can force-resolve; `Viewer`, `Operator`, and `Auditor` cannot.

### Task 3: Implement RBAC Foundation

**Files:**
- Create: `src/authz.rs`
- Modify: `src/lib.rs`

- [x] Add `RbacRole`.
- [x] Add `AdminAction`.
- [x] Add `AuthorizationRequest`.
- [x] Add `AuthorizationPolicy`.
- [x] Add `PermitAllAuthorizationPolicy` for local compatibility.
- [x] Add `StaticAuthorizationPolicy` for tests and product wiring.

### Task 4: Implement Product Audit Store Foundation

**Files:**
- Modify: `src/telemetry/audit.rs`
- Modify: `src/telemetry/mod.rs`

- [x] Add `ProductAuditRecord`.
- [x] Add `ProductAuditStore`.
- [x] Add `NoopProductAuditStore`.
- [x] Add `InMemoryProductAuditStore`.
- [x] Add `FailingProductAuditStore` in the integration test.

### Task 5: Enforce RBAC and Audit on Force Resolve

**Files:**
- Modify: `src/api/service.rs`
- Modify: `src/api/admin_ops.rs`
- Modify: `src/error.rs`

- [x] Add injectable authorization policy and product audit store to `AriaUnderlayService`.
- [x] Pass those dependencies into `AdminOps`.
- [x] Authorize `force_resolve_transaction` before journal mutation.
- [x] Write product audit pre-record before journal mutation.
- [x] Return fail-closed errors for authorization denial and product audit write failure.

### Task 6: Verify and Ship

**Files:**
- Verify all changed files.

- [x] Run `git diff --check`.
- [x] Run `python3 -m pytest adapter-python/tests -q`.
- [ ] Run Rust checks in GitHub Actions because local `cargo` is unavailable.
- [ ] Commit and push the package.
- [ ] Wait for GitHub Actions to pass before reporting completion.
