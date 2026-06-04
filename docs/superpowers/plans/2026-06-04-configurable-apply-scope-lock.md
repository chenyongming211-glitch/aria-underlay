# Configurable Apply Scope Lock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `apply_domain_intent` serialization configurable by domain, region, or switch-pair endpoint scope.

**Architecture:** Extend `ApplyOptions` with a lock-scope enum and optional `region_id`. Resolve each request to deterministic lock keys in the service layer, then extend the existing local async apply lock table to acquire one or more sorted keys. Keep endpoint locks, transaction journal, idempotency, retry compensation, and verify reports unchanged.

**Tech Stack:** Rust, serde, Tokio async mutex, DashMap, existing transaction gate tests.

---

### Task 1: Request Model

**Files:**
- Modify: `src/api/request.rs`
- Test: `tests/request_tests.rs`

- [ ] **Step 1: Write failing request tests**

Add tests:

```rust
#[test]
fn apply_options_default_to_domain_lock_scope() {
    let options = ApplyOptions::default();

    assert_eq!(options.lock_scope, ApplyLockScope::Domain);
    assert!(options.region_id.is_none());
}

#[test]
fn apply_options_parse_region_lock_scope() {
    let options: ApplyOptions = serde_json::from_str(r#"{
        "dry_run": false,
        "allow_degraded_atomicity": false,
        "lock_scope": "Region",
        "region_id": "region-a"
    }"#).unwrap();

    assert_eq!(options.lock_scope, ApplyLockScope::Region);
    assert_eq!(options.region_id.as_deref(), Some("region-a"));
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test lock_scope --test request_tests`

Expected locally: `cargo` is unavailable; CI will verify compile/test failure
before implementation.

- [ ] **Step 3: Implement request model**

Add:

```rust
pub enum ApplyLockScope { Domain, Region, SwitchPair }
```

Add `lock_scope` and `region_id` to `ApplyOptions`, both serde-defaulted.
Default `lock_scope` is `Domain`.

### Task 2: Apply Scope Key Resolution and Lock Table

**Files:**
- Modify: `src/tx/domain_lock.rs`
- Modify: `src/tx/mod.rs`
- Modify: `src/api/service.rs`
- Test: `tests/transaction_tests.rs`

- [ ] **Step 1: Write failing lock tests**

Add tests that prove `DomainApplyLockTable::acquire_many` serializes
overlapping key sets and allows disjoint key sets:

```rust
#[tokio::test]
async fn domain_apply_lock_serializes_overlapping_scope_keys() {
    let locks = DomainApplyLockTable::default();
    let first_guard = locks.acquire_many(["switch_pair:domain-a:stack-a", "switch_pair:domain-a:stack-b"]).await?;
    // second writer with one overlapping key should wait
}
```

Add a helper-facing unit test for lock key resolution:

```rust
#[test]
fn apply_scope_keys_require_region_id_for_region_scope() {
    let err = apply_scope_lock_keys(&request_without_region).unwrap_err();
    assert!(err.to_string().contains("region_id"));
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test domain_apply_lock --test transaction_tests
```

- [ ] **Step 3: Implement multi-key locking**

Extend `DomainApplyLockTable`:

```rust
pub async fn acquire_many<I, S>(&self, keys: I) -> UnderlayResult<DomainApplyGuard>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>;
```

Normalize keys, deduplicate with `BTreeSet`, and acquire sorted keys into a
guard vector. Keep `acquire(domain_id)` as a compatibility wrapper using
`domain:<domain_id>`.

- [ ] **Step 4: Implement scope key resolver**

Add a service helper:

```rust
fn apply_scope_lock_keys(request: &ApplyDomainIntentRequest) -> UnderlayResult<Vec<String>>
```

Rules:

- Domain: `domain:<domain_id>`
- Region: `region:<region_id>`
- SwitchPair: sorted `switch_pair:<domain_id>:<endpoint_id>` for all endpoints.

### Task 3: Service Wiring and Concurrency Tests

**Files:**
- Modify: `src/api/service.rs`
- Test: `tests/transaction_gate_tests.rs`

- [ ] **Step 1: Write failing service tests**

Add tests:

```rust
#[tokio::test]
async fn region_lock_scope_serializes_different_domains_in_same_region() {
    // first request domain-a region-a holds prepare
    // second request domain-b region-a must not reach prepare until first releases
}

#[tokio::test]
async fn switch_pair_lock_scope_allows_disjoint_endpoints_in_same_domain() {
    // two same-domain requests with different endpoint ids and SwitchPair scope
    // second reaches prepare while first is held
}
```

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test lock_scope --test transaction_gate_tests
```

- [ ] **Step 3: Wire service**

Replace the current domain-only guard with:

```rust
let lock_keys = apply_scope_lock_keys(&request)?;
let _scope_guard = self.domain_apply_locks.acquire_many(lock_keys).await?;
```

Keep the guard alive across idempotency reuse, planning, apply, and persistence.

### Task 4: Final Verification and CI

**Files:**
- All modified Rust and docs files

- [ ] **Step 1: Run available local checks**

Run: `git diff --check`

Expected: no whitespace errors.

- [ ] **Step 2: Commit and push**

Commit spec, plan, implementation, and tests. Do not stage untracked
`AGENTS.md`.

- [ ] **Step 3: Wait for CI**

Poll GitHub Actions for the pushed head commit. Do not start P1-6 until CI is
green.
