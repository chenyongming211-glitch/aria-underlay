# PartialSuccess Failed Endpoint Retry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a service-level compensation path that records domain apply results, classifies failed endpoints, and retries only terminal failed endpoints.

**Architecture:** Introduce a focused `api::apply_compensation` module containing the apply record store, compensation plan types, endpoint classification, and sub-intent filtering. `AriaUnderlayService` persists non-reused domain apply records and exposes `get_domain_apply_compensation_plan` plus `retry_failed_domain_endpoints`. Existing endpoint transactions, idempotency, domain locking, journal, and recovery semantics remain unchanged.

**Tech Stack:** Rust, Tokio, serde JSON, existing `ApplyDomainIntentRequest`, `ApplyIntentResponse`, and transaction gate tests.

---

### Task 1: Apply Record Store and Pure Compensation Logic

**Files:**
- Create: `src/api/apply_compensation.rs`
- Modify: `src/api/mod.rs`
- Test: `tests/transaction_tests.rs`

- [ ] **Step 1: Write failing unit tests**

Add tests that expect:

```rust
#[test]
fn compensation_plan_classifies_terminal_failed_and_in_doubt_endpoints() {
    let response = ApplyIntentResponse {
        request_id: "req-original".into(),
        trace_id: "trace-original".into(),
        idempotency_key: None,
        reused: false,
        tx_id: None,
        status: ApplyStatus::PartialSuccess,
        strategy: None,
        device_results: vec![
            device_result("stack-a", ApplyStatus::Success),
            device_result("stack-b", ApplyStatus::RolledBack),
            device_result("stack-c", ApplyStatus::InDoubt),
        ],
        warnings: Vec::new(),
    };

    let plan = DomainApplyCompensationPlan::from_response(&response);

    assert_eq!(plan.completed, vec![DeviceId("stack-a".into())]);
    assert_eq!(plan.retryable_failed, vec![DeviceId("stack-b".into())]);
    assert_eq!(plan.requires_recovery, vec![DeviceId("stack-c".into())]);
}
```

Add a filtering test that builds a two-endpoint `UnderlayDomainIntent`, filters to `stack-b`, and expects only `stack-b`, its member, and member-scoped interface entries to remain.

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test compensation --test transaction_tests
```

Expected: compile failure because `apply_compensation` types do not exist. In this Windows workspace `cargo` is unavailable, so record the local limitation and rely on GitHub Actions.

- [ ] **Step 3: Implement minimal pure logic and store**

Create:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainApplyRecord {
    pub request: ApplyDomainIntentRequest,
    pub response: ApplyIntentResponse,
    pub domain_id: String,
    pub created_at_unix_secs: u64,
    pub updated_at_unix_secs: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainApplyCompensationPlan {
    pub original_request_id: String,
    pub original_trace_id: String,
    pub domain_id: String,
    pub status: ApplyStatus,
    pub retryable_failed: Vec<DeviceId>,
    pub requires_recovery: Vec<DeviceId>,
    pub completed: Vec<DeviceId>,
}
```

Add `InMemoryDomainApplyRecordStore`, `JsonFileDomainApplyRecordStore`, `DomainApplyRecordStore`, `filter_domain_intent_to_endpoints`, and `select_retryable_failed_endpoints`.

- [ ] **Step 4: Verify green**

Run:

```bash
cargo test compensation --test transaction_tests
```

Expected: compensation unit tests pass in CI.

### Task 2: Service Wiring and Retry API

**Files:**
- Modify: `src/api/request.rs`
- Modify: `src/api/response.rs`
- Modify: `src/api/service.rs`
- Test: `tests/transaction_gate_tests.rs`

- [ ] **Step 1: Write failing service integration test**

Add a two-endpoint apply where `stack-a` succeeds and `stack-b` rolls back. Assert:

```rust
let response = service.apply_domain_intent(two_endpoint_request()).await?;
assert_eq!(response.status, ApplyStatus::PartialSuccess);

let plan = service
    .get_domain_apply_compensation_plan("req-original")
    .expect("plan should exist");
assert_eq!(plan.retryable_failed, vec![DeviceId("stack-b".into())]);

let retry_response = service
    .retry_failed_domain_endpoints(RetryFailedDomainEndpointsRequest {
        request_id: "req-retry".into(),
        trace_id: Some("trace-retry".into()),
        original_request_id: "req-original".into(),
        endpoint_ids: Vec::new(),
        idempotency_key: None,
    })
    .await?;

assert_eq!(stack_a_prepare_calls.load(Ordering::SeqCst), 1);
assert_eq!(stack_b_prepare_calls.load(Ordering::SeqCst), 2);
assert_eq!(retry_response.request_id, "req-retry");
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test retry_failed_domain_endpoints --test transaction_gate_tests
```

Expected: compile failure because service methods and request type do not exist. Local workspace lacks `cargo`; GitHub Actions will verify.

- [ ] **Step 3: Wire service**

Add:

```rust
pub struct RetryFailedDomainEndpointsRequest {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub original_request_id: String,
    pub endpoint_ids: Vec<DeviceId>,
    pub idempotency_key: Option<String>,
}
```

Add `domain_apply_records: Arc<dyn DomainApplyRecordStore>` to `AriaUnderlayService`, initialize it in all constructors, add `with_file_domain_apply_record_store`, persist records after domain apply, and implement the two compensation methods.

- [ ] **Step 4: Verify green**

Run:

```bash
cargo test retry_failed_domain_endpoints --test transaction_gate_tests
```

Expected: the integration test passes in CI and proves successful endpoints are not touched by retry.

### Task 3: Persistence and Final Verification

**Files:**
- Test: `tests/transaction_tests.rs`
- Test: `tests/transaction_gate_tests.rs`

- [ ] **Step 1: Add file-backed store round-trip test**

Write a test that stores a `DomainApplyRecord`, recreates `JsonFileDomainApplyRecordStore`, and loads the record by request id.

- [ ] **Step 2: Add service recreation test**

Use `with_file_domain_apply_record_store` for the first service, perform a partial apply, recreate the service with the same store root, and verify `get_domain_apply_compensation_plan("req-original")` still returns the failed endpoint.

- [ ] **Step 3: Run local available checks**

Run:

```bash
git diff --check
```

Expected: no whitespace errors.

- [ ] **Step 4: Commit and push**

Run:

```bash
git add src/api/apply_compensation.rs src/api/mod.rs src/api/request.rs src/api/response.rs src/api/service.rs tests/transaction_tests.rs tests/transaction_gate_tests.rs docs/superpowers/plans/2026-06-04-partial-success-endpoint-retry.md
git commit -m "add failed endpoint retry compensation"
git push origin codex/underlay-transaction-docs-fixes
```

- [ ] **Step 5: Wait for CI**

Poll GitHub Actions for the pushed commit. Do not begin P1-4 until the CI run completes successfully.
