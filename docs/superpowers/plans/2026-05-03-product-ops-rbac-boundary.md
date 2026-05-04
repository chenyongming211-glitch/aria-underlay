# Product Ops RBAC Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable product operations manager that RBAC-gates product-facing reads and audit exports.

**Architecture:** Add `src/api/product_ops.rs` as a focused facade over `AuthorizationPolicy`, `OperationSummaryStore`, and `ProductAuditStore`. Extend `ProductAuditStore` with a read method so audit export can use the same trait boundary as append.

**Tech Stack:** Rust, serde, existing authz, telemetry, and operation summary store traits.

---

### Task 1: Product Ops Tests

**Files:**
- Create: `tests/product_ops_rbac_tests.rs`

- [ ] **Step 1: Write failing tests**

Add tests for summary list authorization, audit export authorization, export audit recording, and audit-write-failure fail-closed behavior.

- [ ] **Step 2: Run focused tests to verify red**

Run:

```bash
cargo test --test product_ops_rbac_tests
```

Expected locally if `cargo` exists: compile failure because `api::product_ops` does not exist yet. If `cargo` is unavailable, record that local Rust execution is blocked and rely on GitHub Actions after implementation.

### Task 2: Product Audit Store Read Boundary

**Files:**
- Modify: `src/telemetry/audit.rs`
- Modify: `src/telemetry/mod.rs`

- [ ] **Step 1: Extend trait**

Add `fn list(&self) -> UnderlayResult<Vec<ProductAuditRecord>>` to `ProductAuditStore`.

- [ ] **Step 2: Implement for stores**

Implement `list` for `NoopProductAuditStore`, `InMemoryProductAuditStore`, and `JsonFileProductAuditStore`.

- [ ] **Step 3: Add audit record constructor**

Add `ProductAuditRecord::product_audit_export_requested`.

### Task 3: Product Ops Manager

**Files:**
- Create: `src/api/product_ops.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Add request/response types**

Add `ProductOperatorContext`, `ExportProductAuditRequest`, `ProductAuditExportOverview`, and `ExportProductAuditResponse`.

- [ ] **Step 2: Add manager methods**

Implement `list_operation_summaries` and `export_product_audit`.

- [ ] **Step 3: Wire module export**

Add `pub mod product_ops;` in `src/api/mod.rs`.

### Task 4: Docs

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`

- [ ] **Step 1: Document product API boundary**

Record that product-facing reads now require operator context and RBAC.

- [ ] **Step 2: Update remaining gaps**

Leave real HTTP routing, internal identity wiring, UI, and online reload as open work.

### Task 5: Verification and Release Gate

**Files:**
- All changed files

- [ ] **Step 1: Run local runnable checks**

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests -q
cargo test --test product_ops_rbac_tests
```

- [ ] **Step 2: Commit and push**

Stage only relevant files, commit, push to `origin main`, and wait for GitHub Actions to pass before moving to the next package.
