# HA Lease Fail-Closed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a production startup gate that refuses write-capable service startup unless the required active-passive lease is configured and acquired.

**Architecture:** Reuse `ActiveLeaseGuard` and `ActivePassiveAriaUnderlayService`. Add an HA startup policy and service enum in `src/api/service.rs`, then expose the types from `src/api/mod.rs`. The enum implements `UnderlayService` by delegating to either the standalone service or the active-passive wrapper.

**Tech Stack:** Rust, async_trait, existing HA file lease, existing service trait, GitHub Actions CI.

---

### Task 1: Startup Policy Types

**Files:**
- Modify: `src/api/service.rs`
- Modify: `src/api/mod.rs`
- Test: `tests/ha_tests.rs`

- [ ] **Step 1: Write failing tests**

Add tests:

```rust
#[tokio::test]
async fn ha_required_startup_fails_without_lease_config() {
    let err = AriaUnderlayService::new(DeviceInventory::default())
        .activate_with_ha_policy(HaLeaseStartupPolicy::require_active_lease(None))
        .await
        .expect_err("production HA mode must fail closed without lease config");

    assert_adapter_code(err, "HA_LEASE_REQUIRED");
}

#[tokio::test]
async fn ha_standalone_allowed_startup_returns_plain_service() {
    let service = AriaUnderlayService::new(DeviceInventory::default())
        .activate_with_ha_policy(HaLeaseStartupPolicy::standalone_allowed())
        .await
        .expect("local mode should not require a lease");

    assert!(service.is_standalone());
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test ha_required_startup_fails_without_lease_config ha_standalone_allowed_startup_returns_plain_service --test ha_tests
```

Expected locally: `cargo` is unavailable; CI verifies compile/test behavior.

- [ ] **Step 3: Implement policy and enum**

Add:

```rust
pub enum HaLeaseMode {
    StandaloneAllowed,
    RequireActiveLease,
}

pub struct HaLeaseStartupPolicy {
    pub mode: HaLeaseMode,
    pub lease_config: Option<ActiveLeaseConfig>,
}

pub enum HaProtectedAriaUnderlayService {
    Standalone(AriaUnderlayService),
    ActivePassive(ActivePassiveAriaUnderlayService),
}
```

Implement constructors and `AriaUnderlayService::activate_with_ha_policy(policy)`.

### Task 2: Required Lease Behavior

**Files:**
- Modify: `src/api/service.rs`
- Test: `tests/ha_tests.rs`

- [ ] **Step 1: Write failing tests**

Add tests:

```rust
#[tokio::test]
async fn ha_required_startup_acquires_lease_and_runs_recovery() {
    let path = temp_lease_path("required-active");
    let service = AriaUnderlayService::new(DeviceInventory::default())
        .activate_with_ha_policy(HaLeaseStartupPolicy::require_active_lease(Some(
            ActiveLeaseConfig::new(&path, "node-a").with_heartbeat_interval_secs(1),
        )))
        .await
        .expect("required HA mode should acquire free lease");

    assert!(service.is_active_passive());
}

#[tokio::test]
async fn ha_required_startup_rejects_second_active() {
    let path = temp_lease_path("required-held");
    let _active = AriaUnderlayService::new(DeviceInventory::default())
        .activate_with_ha_policy(HaLeaseStartupPolicy::require_active_lease(Some(
            ActiveLeaseConfig::new(&path, "node-a").with_heartbeat_interval_secs(1),
        )))
        .await
        .expect("first active service should acquire lease");

    let err = AriaUnderlayService::new(DeviceInventory::default())
        .activate_with_ha_policy(HaLeaseStartupPolicy::require_active_lease(Some(
            ActiveLeaseConfig::new(&path, "node-b").with_heartbeat_interval_secs(1),
        )))
        .await
        .expect_err("second active service should be rejected");

    assert_adapter_code(err, "HA_LEASE_HELD");
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test ha_required_startup --test ha_tests
```

- [ ] **Step 3: Implement required mode**

For required mode, if `lease_config` is `None`, return an `AdapterOperation` with code `HA_LEASE_REQUIRED`, retryable false, and a message explaining that production HA requires active lease config. If present, call `activate_active_passive`.

### Task 3: Service Trait Delegation and Documentation

**Files:**
- Modify: `src/api/service.rs`
- Modify: `docs/runbooks/active-passive-ha.md`
- Test: `tests/ha_tests.rs`

- [ ] **Step 1: Write failing delegation test**

Add a simple local-mode delegation test:

```rust
#[tokio::test]
async fn ha_protected_standalone_service_delegates_read_ops() {
    let service = AriaUnderlayService::new(DeviceInventory::default())
        .activate_with_ha_policy(HaLeaseStartupPolicy::standalone_allowed())
        .await
        .unwrap();

    let response = service
        .list_operation_summaries(ListOperationSummariesRequest::default())
        .await
        .unwrap();
    assert_eq!(response.summaries.len(), 0);
}
```

- [ ] **Step 2: Implement `UnderlayService` for enum**

Delegate each trait method to the inner standalone or active-passive service.

- [ ] **Step 3: Update runbook**

Document that production launchers should call `activate_with_ha_policy(HaLeaseStartupPolicy::require_active_lease(Some(...)))`, and that `standalone_allowed()` is for local/test/single-node mode only.

### Task 4: Verification and CI

**Files:**
- All modified files

- [ ] **Step 1: Run available local checks**

Run:

```bash
git diff --check
```

- [ ] **Step 2: Commit and push**

Do not stage untracked `AGENTS.md`.

- [ ] **Step 3: Wait for GitHub Actions**

Poll the pushed commit's CI run. Fix failures until the full run is green.
