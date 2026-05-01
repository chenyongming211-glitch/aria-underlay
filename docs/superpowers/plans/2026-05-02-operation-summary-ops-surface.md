# Operation Summary Ops Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development for code changes and superpowers:verification-before-completion before claiming completion.

**Goal:** Make persisted operation summaries usable as an offline operator-facing query surface without requiring real switches, device sessions, or a product UI.

**Architecture:** Extend the existing `list_operation_summaries` service API rather than adding a parallel JSONL parser. The service remains the single query path over any `OperationSummaryStore`. It filters records, returns a matched-record overview before limit truncation, and the existing `transaction_ops` example exposes read-only JSON output for local operations.

**Scope:**

- Add result filtering to `ListOperationSummariesRequest`.
- Add `OperationSummaryOverview` with matched/returned counts and action/result/device rollups.
- Keep `limit` as a display concern: it truncates returned records, not the overview counts.
- Extend `examples/transaction_ops.rs` with `list-operations` and `operation-summary`.
- Keep storage, authorization, alert delivery, and product UI as separate follow-up work.

---

### Task 1: API Rollup

**Files:**

- Modify: `src/api/operations.rs`
- Modify: `src/api/service.rs`
- Test: `tests/telemetry_tests.rs`

- [x] **Step 1: Write failing test**

Add a test that records multiple operation events, filters by result, applies a limit, and expects the overview to count all filtered records before limit truncation.

Local note: `cargo test operation_summary_query_returns_rollup_for_all_filtered_records_before_limit` cannot run in this workspace because `cargo` is unavailable.

- [x] **Step 2: Implement request/response changes**

Add `result` to `ListOperationSummariesRequest` and add `OperationSummaryOverview` to `ListOperationSummariesResponse`.

- [x] **Step 3: Preserve limit semantics**

Apply filters first, compute overview over the full filtered set, then apply `limit` to returned records.

### Task 2: Offline CLI Entry

**Files:**

- Modify: `examples/transaction_ops.rs`

- [x] **Step 1: Add list command**

Add:

```text
cargo run --example transaction_ops -- list-operations --operation-summary-path <file> [filters]
```

It prints the full response: matching summaries plus overview.

- [x] **Step 2: Add overview-only command**

Add:

```text
cargo run --example transaction_ops -- operation-summary --operation-summary-path <file> [filters]
```

It prints only the overview object.

- [x] **Step 3: Reuse service API**

Both commands call `AriaUnderlayService::list_operation_summaries()` with a `JsonFileOperationSummaryStore`; they do not parse or mutate JSONL directly.

### Task 3: Verification

- [x] **Step 1: Local checks**

Run:

```bash
python3 -m pytest adapter-python/tests -q
git diff --check
```

Result: Python adapter tests passed with `238 passed`; `git diff --check` exited cleanly.

- [x] **Step 2: Rust checks**

If local Rust remains unavailable, push and use GitHub Actions as the Rust verification gate.

Result: `cargo test operation_summary_query_returns_rollup_for_all_filtered_records_before_limit` returned `zsh:1: command not found: cargo`, so Rust verification is deferred to GitHub Actions.

- [ ] **Step 3: Commit and push**

Commit this package, push it, and wait for GitHub Actions to pass before starting the next package.
