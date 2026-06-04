# Apply Verify Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose transaction-scoped post-commit verify results as product-visible `verify_report` fields on device and apply responses.

**Architecture:** Add verify report structs to `src/api/response.rs`, then populate them in `ApplyCoordinator` where verify is already executed. Keep the adapter/protobuf contract unchanged and build scope summaries from existing change sets. Aggregation remains response-local, so domain apply records automatically persist the report.

**Tech Stack:** Rust, serde, existing Tokio transaction tests, existing adapter test fixture.

---

### Task 1: Response Model and Aggregation

**Files:**
- Modify: `src/api/response.rs`
- Modify: `src/api/apply.rs`
- Test: `tests/transaction_tests.rs`

- [ ] **Step 1: Write failing tests**

Add tests that create `DeviceApplyResult` values with verify reports and assert aggregate status:

```rust
#[test]
fn aggregate_verify_report_marks_partial_when_some_endpoints_fail_verify() {
    let report = ApplyVerifyReport::from_device_results(&[
        device_result_with_verify("leaf-a", DeviceVerifyStatus::Passed),
        device_result_with_verify("leaf-b", DeviceVerifyStatus::Failed),
    ]);

    assert_eq!(report.status, ApplyVerifyStatus::Partial);
    assert_eq!(report.passed, vec![DeviceId("leaf-a".into())]);
    assert_eq!(report.failed, vec![DeviceId("leaf-b".into())]);
    assert!(report.attention_required);
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test verify_report --test transaction_tests
```

Expected locally: compile failure before report structs exist. In this Windows workspace, `cargo` is unavailable, so GitHub Actions is the compile gate.

- [ ] **Step 3: Implement model**

Add:

```rust
pub enum DeviceVerifyStatus { Passed, Failed, Skipped, InDoubt }
pub enum ApplyVerifyStatus { Passed, Failed, Partial, Skipped, InDoubt }
pub struct VerifyScopeSummary { vlan_count, interface_count, acl_count, acl_binding_count, delete_* counts }
pub struct DeviceVerifyReport { device_id, status, source, scope, warnings, error_code, error_message }
pub struct ApplyVerifyReport { status, passed, failed, skipped, in_doubt, attention_required, warning_count }
```

Use `#[serde(default, skip_serializing_if = "Option::is_none")]` for optional report fields on existing response structs.

### Task 2: Apply Coordinator Wiring

**Files:**
- Modify: `src/api/apply_coordinator.rs`
- Modify: `src/api/apply.rs`
- Test: `tests/transaction_gate_tests.rs`

- [ ] **Step 1: Write failing integration tests**

Add tests:

```rust
#[tokio::test]
async fn successful_apply_returns_scoped_verify_report() {
    let response = service.apply_domain_intent(apply_request_with_vlan(200, DriftPolicy::ReportOnly)).await?;
    let report = response.device_results[0].verify_report.as_ref().unwrap();
    assert_eq!(report.status, DeviceVerifyStatus::Passed);
    assert_eq!(report.scope.vlan_count, 1);
    assert_eq!(response.verify_report.unwrap().status, ApplyVerifyStatus::Passed);
}

#[tokio::test]
async fn verify_failure_returns_failed_verify_report() {
    let response = service.apply_domain_intent(request).await?;
    let report = response.device_results[0].verify_report.as_ref().unwrap();
    assert_eq!(report.status, DeviceVerifyStatus::Failed);
    assert_eq!(report.error_code.as_deref(), Some("VERIFY_FAILED"));
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test verify_report --test transaction_gate_tests
```

Expected locally: compile failure before response fields exist. CI verifies.

- [ ] **Step 3: Populate reports**

Change `verify_endpoint` to return adapter outcome on success and a structured error carrying `DeviceVerifyReport` on failure. Keep transaction phase handling unchanged. Add helper functions:

```rust
fn passed_verify_report(device_id, change_set, outcome) -> DeviceVerifyReport
fn failed_verify_report(device_id, change_set, error) -> DeviceVerifyReport
fn skipped_verify_report(device_id) -> DeviceVerifyReport
fn in_doubt_verify_report(device_id, error) -> DeviceVerifyReport
```

Attach the report to the final `DeviceApplyResult`.

### Task 3: Compatibility and Final Verification

**Files:**
- Modify tests touched above

- [ ] **Step 1: Add serde compatibility test**

Deserialize a legacy `ApplyIntentResponse` JSON without `verify_report` and assert both report fields are `None`.

- [ ] **Step 2: Run available local checks**

Run:

```bash
git diff --check
```

Expected: no whitespace errors.

- [ ] **Step 3: Commit and push**

Run:

```bash
git add src/api/response.rs src/api/apply.rs src/api/apply_coordinator.rs tests/transaction_tests.rs tests/transaction_gate_tests.rs docs/superpowers/plans/2026-06-04-apply-verify-report.md
git commit -m "add apply verify reports"
git push origin codex/underlay-transaction-docs-fixes
```

- [ ] **Step 4: Wait for CI**

Poll the GitHub Actions run for the pushed commit. Do not start P1-5 until CI is green.
