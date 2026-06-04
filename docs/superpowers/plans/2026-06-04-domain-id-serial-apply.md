# Domain ID Serial Apply Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serialize `apply_domain_intent` calls by `UnderlayDomainIntent.domain_id`.

**Architecture:** Add a local async `DomainApplyLockTable` next to endpoint locking, then acquire it at the start of `AriaUnderlayService::apply_domain_intent`. Endpoint locks, journal semantics, idempotency reuse, and drift checks remain unchanged.

**Tech Stack:** Rust, Tokio async mutex, DashMap, existing transaction tests.

---

### Task 1: Domain Lock Table

**Files:**
- Create: `src/tx/domain_lock.rs`
- Modify: `src/tx/mod.rs`
- Test: `tests/transaction_tests.rs`

- [ ] **Step 1: Write failing tests**

Add tests next to existing endpoint lock tests:

```rust
#[tokio::test]
async fn domain_apply_lock_serializes_same_domain_writers() {
    let locks = DomainApplyLockTable::default();
    let first_guard = locks
        .acquire("domain-a")
        .await
        .expect("first domain lock should be acquired");
    let acquired = Arc::new(AtomicBool::new(false));
    let second_acquired = acquired.clone();
    let second_locks = locks.clone();

    let second = tokio::spawn(async move {
        let _guard = second_locks
            .acquire("domain-a")
            .await
            .expect("second domain lock should eventually be acquired");
        second_acquired.store(true, Ordering::SeqCst);
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!acquired.load(Ordering::SeqCst));

    drop(first_guard);
    second.await.expect("second lock task should finish");
    assert!(acquired.load(Ordering::SeqCst));
}

#[tokio::test]
async fn domain_apply_lock_allows_different_domains_to_run_concurrently() {
    let locks = DomainApplyLockTable::default();
    let _first_guard = locks
        .acquire("domain-a")
        .await
        .expect("first domain lock should be acquired");

    let _second_guard = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        locks.acquire("domain-b"),
    )
    .await
    .expect("different domain should not wait")
    .expect("second domain lock should be acquired");
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test domain_apply_lock --test transaction_tests`

Expected locally: this fails because `DomainApplyLockTable` does not exist. In the current Windows workspace, `cargo` is unavailable, so GitHub Actions will be used for the actual compile check.

- [ ] **Step 3: Implement minimal lock table**

Create:

```rust
#[derive(Debug, Clone, Default)]
pub struct DomainApplyLockTable {
    locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

#[derive(Debug)]
pub struct DomainApplyGuard {
    _guard: OwnedMutexGuard<()>,
}

impl DomainApplyLockTable {
    pub async fn acquire(&self, domain_id: &str) -> UnderlayResult<DomainApplyGuard> {
        let key = domain_lock_key(domain_id);
        let lock = self.lock_for(key);
        Ok(DomainApplyGuard {
            _guard: lock.lock_owned().await,
        })
    }
}
```

- [ ] **Step 4: Verify green**

Run: `cargo test domain_apply_lock --test transaction_tests`

Expected: the two domain lock table tests pass.

### Task 2: Service Apply Wiring

**Files:**
- Modify: `src/api/service.rs`
- Test: `tests/transaction_gate_tests.rs`

- [ ] **Step 1: Write failing service tests**

Add tests that use two concurrent `apply_domain_intent` calls:

```rust
#[tokio::test]
async fn apply_domain_intent_serializes_same_domain_requests() {
    // Start first request, hold adapter prepare, then start second same-domain request.
    // Assert second request does not reach prepare until first is released.
}

#[tokio::test]
async fn apply_domain_intent_allows_different_domains_to_progress_independently() {
    // Hold domain-a prepare, start domain-b apply, assert domain-b reaches prepare.
}
```

- [ ] **Step 2: Verify red**

Run: `cargo test domain_intent_.*domain --test transaction_gate_tests`

Expected: same-domain serialization test fails because both requests can reach adapter prepare without a domain lock.

- [ ] **Step 3: Wire lock into service**

Add `domain_apply_locks: DomainApplyLockTable` to `AriaUnderlayService`, initialize it in constructors, and acquire it at the start of `apply_domain_intent`:

```rust
let _domain_guard = self
    .domain_apply_locks
    .acquire(&request.intent.domain_id)
    .await?;
```

Keep this guard alive through idempotency lookup, planning, apply, idempotency persistence, and return.

- [ ] **Step 4: Verify green**

Run: `cargo test apply_domain_intent_serializes_same_domain_requests apply_domain_intent_allows_different_domains_to_progress_independently --test transaction_gate_tests`

Expected: both service concurrency tests pass, and existing idempotency tests still pass.

### Task 3: Final Verification

**Files:**
- All modified Rust files and tests

- [ ] **Step 1: Run local static checks**

Run: `git diff --check`

Expected: no whitespace errors.

- [ ] **Step 2: Commit and push**

Run:

```bash
git add src/tx/domain_lock.rs src/tx/mod.rs src/api/service.rs tests/transaction_tests.rs tests/transaction_gate_tests.rs docs/superpowers/plans/2026-06-04-domain-id-serial-apply.md
git commit -m "add domain apply serialization"
git push origin codex/underlay-transaction-docs-fixes
```

- [ ] **Step 3: Verify CI**

Use GitHub Actions for full Rust/Python/offline acceptance verification.
