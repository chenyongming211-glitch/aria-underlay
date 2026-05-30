# Transaction Phase State Machine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Centralize production transaction phase changes behind a validated `TxJournalRecord::transition_phase()` API without changing journal format or current recovery semantics.

**Architecture:** Keep `TxPhase` as the serialized enum and keep existing journal stores unchanged. Add a focused transition matrix in `src/tx/phase_transition.rs`, expose a record-level `transition_phase()` method from `src/tx/journal.rs`, and migrate production apply/recovery/admin paths away from direct `.with_phase()` phase mutation. Tests keep using fixture builders or `.with_phase()` until a later public-field encapsulation pass.

**Tech Stack:** Rust 2021, existing `thiserror` error enum, existing `TxJournalRecord` JSON serde model, GitHub Actions for Rust `cargo check` / `cargo test` because local `cargo` is unavailable.

---

## Scope

This plan implements Phase 1 only. It does not implement Product HTTP TLS, worker event bus, HA journal replication, WAL journal format, non-`:candidate` atomicity, or public-field encapsulation of `TxJournalRecord`.

`Committed -> InDoubt` remains legal because the current apply path can commit on the adapter and then fail while persisting shadow state. `ForceResolved` remains legal only from `InDoubt`.

## File Structure

- Modify: `src/error.rs`
  Holds `UnderlayError`; make invalid phase transition structured enough for callers and tests.
- Modify: `src/tx/phase_transition.rs`
  Owns the phase transition matrix and matrix tests. This file already exists as WIP.
- Modify: `src/tx/journal.rs`
  Adds `TxJournalRecord::transition_phase()` and keeps `with_phase()` available for tests and fixture setup.
- Modify: `src/tx/mod.rs`
  Re-exports `validate_transition` and, if useful, transition error helpers.
- Modify: `src/api/apply_coordinator.rs`
  Migrates production phase changes in apply, commit, verify, final-confirm, rollback, success and failure handling.
- Modify: `src/api/recovery_coordinator.rs`
  Migrates production phase changes in recovery scanning and recovery outcomes.
- Modify: `src/api/admin_ops.rs`
  Migrates force-resolve to validated transition after the existing `InDoubt` check.
- Modify: `tests/transaction_tests.rs`
  Adds focused tests for `transition_phase()` success/failure and preserves error history behavior.
- Modify: `tests/recovery_tests.rs`
  Adds or updates recovery regression coverage where invalid transitions would otherwise break recovery.
- Modify: `tests/transaction_process_chaos_tests.rs`
  Adds or updates one end-to-end recovery/rollback test if existing coverage does not hit `Committed -> InDoubt`.
- Modify: `TODOS.md`
  Mark Phase 1 details current after implementation.

---

### Task 1: Normalize The Transition Error And Matrix

**Files:**
- Modify: `src/error.rs`
- Modify: `src/tx/phase_transition.rs`
- Test: `src/tx/phase_transition.rs`

- [ ] **Step 1: Make the invalid transition error include source and target phase names**

Update `src/error.rs` to keep the simple error enum dependency-free from `TxPhase` while preserving structured names:

```rust
#[error("invalid phase transition: {from} -> {to}")]
InvalidPhaseTransition { from: String, to: String },
```

- [ ] **Step 2: Update `validate_transition()` to produce the structured error**

In `src/tx/phase_transition.rs`, replace the current string-only construction with:

```rust
pub fn validate_transition(from: &TxPhase, to: &TxPhase) -> Result<(), UnderlayError> {
    if is_allowed(from, to) {
        Ok(())
    } else {
        Err(UnderlayError::InvalidPhaseTransition {
            from: format!("{from:?}"),
            to: format!("{to:?}"),
        })
    }
}
```

- [ ] **Step 3: Keep the allowed transition matrix explicit**

Ensure `is_allowed()` contains exactly these special rules:

```rust
matches!(
    (from, to),
    (Started, Preparing)
        | (Preparing, Prepared)
        | (Prepared, Committing)
        | (Committing, Verifying)
        | (Verifying, FinalConfirming)
        | (FinalConfirming, Committed)
        | (Preparing | Prepared | Committing | Verifying | FinalConfirming, Failed)
        | (Preparing | Prepared | Committing | Verifying | FinalConfirming, RollingBack)
        | (
            Started | Preparing | Prepared | Committing | Verifying | FinalConfirming | Recovering,
            Recovering
        )
        | (Recovering, Committed | RolledBack | InDoubt)
        | (RollingBack, RolledBack | InDoubt)
        | (Committed, InDoubt)
        | (InDoubt, ForceResolved)
)
```

Leave `from == to` allowed at the top of `is_allowed()`.

- [ ] **Step 4: Run the focused Rust test in CI**

Local command if cargo exists:

```bash
cargo test tx::phase_transition
```

Expected: all `phase_transition` module tests pass.

If local `cargo` is unavailable, commit and push the branch, then use GitHub Actions `cargo test` as the gate.

- [ ] **Step 5: Commit**

```bash
git add src/error.rs src/tx/phase_transition.rs src/tx/mod.rs
git commit -m "test: define transaction phase transition matrix"
```

---

### Task 2: Add Record-Level `transition_phase()`

**Files:**
- Modify: `src/tx/journal.rs`
- Test: `tests/transaction_tests.rs`

- [ ] **Step 1: Add failing tests for record-level transitions**

In `tests/transaction_tests.rs`, add:

```rust
#[test]
fn journal_record_transition_phase_updates_phase_and_timestamp() {
    let context = TxContext {
        tx_id: "tx-transition".into(),
        request_id: "req-transition".into(),
        trace_id: "trace-transition".into(),
    };
    let mut record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);
    let original_updated_at = record.updated_at_unix_secs;

    record
        .transition_phase(TxPhase::Preparing)
        .expect("Started -> Preparing should be valid");

    assert_eq!(record.phase, TxPhase::Preparing);
    assert!(record.updated_at_unix_secs >= original_updated_at);
}

#[test]
fn journal_record_transition_phase_rejects_invalid_skip() {
    let context = TxContext {
        tx_id: "tx-invalid-transition".into(),
        request_id: "req-invalid-transition".into(),
        trace_id: "trace-invalid-transition".into(),
    };
    let mut record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    let err = record
        .transition_phase(TxPhase::Committed)
        .expect_err("Started -> Committed should be invalid");

    assert_eq!(record.phase, TxPhase::Started);
    assert!(err.to_string().contains("Started -> Committed"));
}

#[test]
fn journal_record_transition_phase_preserves_committed_to_in_doubt_recovery_semantics() {
    let context = TxContext {
        tx_id: "tx-committed-shadow-failure".into(),
        request_id: "req-committed-shadow-failure".into(),
        trace_id: "trace-committed-shadow-failure".into(),
    };
    let mut record = TxJournalRecord::started(&context, vec![DeviceId("leaf-a".into())]);

    record.transition_phase(TxPhase::Preparing).unwrap();
    record.transition_phase(TxPhase::Prepared).unwrap();
    record.transition_phase(TxPhase::Committing).unwrap();
    record.transition_phase(TxPhase::Verifying).unwrap();
    record.transition_phase(TxPhase::FinalConfirming).unwrap();
    record.transition_phase(TxPhase::Committed).unwrap();
    record
        .transition_phase(TxPhase::InDoubt)
        .expect("Committed -> InDoubt is required for post-commit shadow failure");

    assert_eq!(record.phase, TxPhase::InDoubt);
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test journal_record_transition_phase --test transaction_tests
```

Expected before implementation: compile failure or method-not-found failure for `transition_phase`.

- [ ] **Step 3: Implement `transition_phase()` on `TxJournalRecord`**

In `src/tx/journal.rs`, import `validate_transition` and add the method next to `with_phase()`:

```rust
use crate::tx::phase_transition::validate_transition;
```

```rust
pub fn transition_phase(&mut self, phase: TxPhase) -> UnderlayResult<()> {
    validate_transition(&self.phase, &phase)?;
    self.phase = phase;
    self.updated_at_unix_secs = now_unix_secs();
    Ok(())
}
```

Keep `with_phase()` public for test fixture construction in this phase.

- [ ] **Step 4: Run the focused test and verify it passes**

Run:

```bash
cargo test journal_record_transition_phase --test transaction_tests
```

Expected: the three new tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tx/journal.rs tests/transaction_tests.rs
git commit -m "feat: add validated transaction phase transitions"
```

---

### Task 3: Migrate Apply Coordinator Production Phase Writes

**Files:**
- Modify: `src/api/apply_coordinator.rs`
- Test: existing apply/recovery transaction tests

- [ ] **Step 1: Replace the initial `Preparing` transition**

Replace:

```rust
journal_record = journal_record.with_phase(TxPhase::Preparing);
```

with:

```rust
if let Err(err) = journal_record.transition_phase(TxPhase::Preparing) {
    return device_error_result(
        &desired.device_id,
        true,
        Some(tx_context.tx_id),
        None,
        err,
    );
}
```

Keep the following `self.journal.put(&journal_record)` unchanged.

- [ ] **Step 2: Replace successful terminal transition without losing strategy**

Replace:

```rust
journal_record = journal_record
    .with_strategy(strategy)
    .with_phase(TxPhase::Committed);
```

with:

```rust
journal_record = journal_record.with_strategy(strategy);
if let Err(err) = journal_record.transition_phase(TxPhase::Committed) {
    let (code, message) = journal_error_fields(&err);
    return DeviceApplyResult {
        device_id: desired.device_id.clone(),
        changed: true,
        status: ApplyStatus::InDoubt,
        tx_id: Some(tx_context.tx_id),
        strategy: Some(strategy),
        error_code: Some(code),
        error_message: Some(message),
        warnings,
    };
}
```

- [ ] **Step 3: Replace post-commit `InDoubt` shadow failure transitions**

For each post-commit shadow/change-set failure block currently using:

```rust
journal_record = journal_record
    .with_phase(TxPhase::InDoubt)
    .with_error(code.clone(), error_message.clone());
```

replace with:

```rust
if let Err(transition_err) = journal_record.transition_phase(TxPhase::InDoubt) {
    let (_, transition_message) = journal_error_fields(&transition_err);
    return DeviceApplyResult {
        device_id: desired.device_id.clone(),
        changed: true,
        status: ApplyStatus::InDoubt,
        tx_id: Some(tx_context.tx_id),
        strategy: Some(strategy),
        error_code: Some(code),
        error_message: Some(format!(
            "{error_message}; phase transition also failed: {transition_message}"
        )),
        warnings,
    };
}
journal_record = journal_record.with_error(code.clone(), error_message.clone());
```

Apply this pattern to missing change-set, shadow read failure, and shadow write failure after adapter commit.

- [ ] **Step 4: Replace `finish_failed_apply()` transition**

Replace:

```rust
journal_record = journal_record
    .with_phase(phase.clone())
    .with_error(code.clone(), message.clone());
```

with:

```rust
if let Err(transition_err) = journal_record.transition_phase(phase.clone()) {
    let (_, transition_message) = journal_error_fields(&transition_err);
    return DeviceApplyResult {
        device_id: desired.device_id.clone(),
        changed: true,
        status: ApplyStatus::InDoubt,
        tx_id: Some(tx_context.tx_id),
        strategy: journal_record.strategy,
        error_code: Some(code),
        error_message: Some(format!(
            "{message}; phase transition also failed: {transition_message}"
        )),
        warnings: Vec::new(),
    };
}
journal_record = journal_record.with_error(code.clone(), message.clone());
```

- [ ] **Step 5: Replace mutable endpoint phase writes**

For each mutable record assignment:

```rust
*journal_record = journal_record.clone().with_phase(TxPhase::Prepared);
self.journal.put(journal_record)?;
```

replace with:

```rust
journal_record.transition_phase(TxPhase::Prepared)?;
self.journal.put(journal_record)?;
```

Repeat for:

- `Prepared`
- `Committing`
- `Verifying`
- `FinalConfirming`
- `RollingBack`
- `RolledBack`

- [ ] **Step 6: Replace rollback failure `InDoubt` writes**

Replace rollback failure blocks that currently clone and call `.with_phase(TxPhase::InDoubt).with_error(...)` with:

```rust
journal_record.transition_phase(TxPhase::InDoubt)?;
*journal_record = journal_record.clone().with_error(code, message);
self.journal.put(journal_record)?;
```

For the unexpected rollback status block, use `"UNEXPECTED_ROLLBACK_STATUS"` and the existing formatted message.

- [ ] **Step 7: Run apply-related tests**

Run:

```bash
cargo test apply --test transaction_process_chaos_tests
cargo test transaction --test transaction_tests
cargo test recovery --test recovery_tests
```

Expected: all selected tests pass. If local `cargo` is unavailable, push and rely on GitHub Actions.

- [ ] **Step 8: Commit**

```bash
git add src/api/apply_coordinator.rs
git commit -m "refactor: validate apply transaction phase changes"
```

---

### Task 4: Migrate Recovery Coordinator And Admin Force Resolve

**Files:**
- Modify: `src/api/recovery_coordinator.rs`
- Modify: `src/api/admin_ops.rs`
- Test: `tests/recovery_tests.rs`
- Test: `tests/ops_cli_tests.rs`

- [ ] **Step 1: Migrate manual intervention `InDoubt` transition**

Replace:

```rust
self.journal
    .put(&record.clone().with_phase(TxPhase::InDoubt))?;
```

with:

```rust
let mut updated = record.clone();
updated.transition_phase(TxPhase::InDoubt)?;
self.journal.put(&updated)?;
```

- [ ] **Step 2: Migrate `Recovering` transition**

Replace:

```rust
self.journal
    .put(&record.clone().with_phase(TxPhase::Recovering))?;
```

with:

```rust
let mut recovering = record.clone();
recovering.transition_phase(TxPhase::Recovering)?;
self.journal.put(&recovering)?;
```

- [ ] **Step 3: Migrate recovery outcome transition**

Replace:

```rust
self.journal.put(&record.clone().with_phase(phase))?;
```

with:

```rust
let mut recovered_record = recovering.clone();
recovered_record.transition_phase(phase.clone())?;
self.journal.put(&recovered_record)?;
```

Use `recovering.clone()` so `Recovering -> Committed`, `Recovering -> RolledBack`, and `Recovering -> InDoubt` follow the matrix instead of jumping from the original phase.

- [ ] **Step 4: Migrate recovery error `InDoubt` writes**

Replace each:

```rust
self.journal.put(
    &record
        .clone()
        .with_phase(TxPhase::InDoubt)
        .with_error(code, message),
)?;
```

with:

```rust
let mut updated = recovering.clone();
updated.transition_phase(TxPhase::InDoubt)?;
updated = updated.with_error(code, message);
self.journal.put(&updated)?;
```

If the code is in a branch before `recovering` exists, clone `record`, transition it to `InDoubt`, then add the error.

- [ ] **Step 5: Migrate force-resolve**

In `src/api/admin_ops.rs`, replace:

```rust
let resolved = record
    .clone()
    .with_manual_resolution(...)
    .with_phase(TxPhase::ForceResolved);
```

with:

```rust
let mut resolved = record.clone().with_manual_resolution(
    request.operator.clone(),
    request.reason.clone(),
    request.request_id.clone(),
    trace_id.clone(),
);
resolved.transition_phase(TxPhase::ForceResolved)?;
```

The existing `if record.phase != TxPhase::InDoubt` guard stays in place.

- [ ] **Step 6: Run recovery/admin tests**

Run:

```bash
cargo test force_resolve --test recovery_tests
cargo test recover_pending_transactions --test recovery_tests
cargo test force_resolve --test ops_cli_tests
```

Expected: selected tests pass. If local `cargo` is unavailable, push and use GitHub Actions.

- [ ] **Step 7: Commit**

```bash
git add src/api/recovery_coordinator.rs src/api/admin_ops.rs tests/recovery_tests.rs tests/ops_cli_tests.rs
git commit -m "refactor: validate recovery phase changes"
```

---

### Task 5: Add Guard Tests For Remaining Production `.with_phase()` Usage

**Files:**
- Modify: tests or add: `tests/phase_transition_usage_tests.rs`

- [ ] **Step 1: Add a source guard test**

Create `tests/phase_transition_usage_tests.rs`:

```rust
use std::fs;
use std::path::Path;

#[test]
fn production_code_does_not_call_with_phase_directly() {
    let roots = ["src/api", "src/worker", "src/tx/recovery.rs"];
    let mut offenders = Vec::new();

    for root in roots {
        collect_with_phase_calls(Path::new(root), &mut offenders);
    }

    assert!(
        offenders.is_empty(),
        "production code must use TxJournalRecord::transition_phase instead of with_phase: {offenders:?}"
    );
}

fn collect_with_phase_calls(path: &Path, offenders: &mut Vec<String>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read source directory") {
            let entry = entry.expect("read source entry");
            collect_with_phase_calls(&entry.path(), offenders);
        }
        return;
    }

    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return;
    }

    let content = fs::read_to_string(path).expect("read source file");
    for (index, line) in content.lines().enumerate() {
        if line.contains(".with_phase(") {
            offenders.push(format!("{}:{}", path.display(), index + 1));
        }
    }
}
```

- [ ] **Step 2: Run the guard test and verify it fails before all migrations are complete**

Run:

```bash
cargo test production_code_does_not_call_with_phase_directly --test phase_transition_usage_tests
```

Expected before migration completion: failure listing remaining production `.with_phase()` callers.

- [ ] **Step 3: Finish any missed production migrations**

Use:

```bash
rg -n "\.with_phase\(" src/api src/worker src/tx
```

Expected after migration: no production hits except tests or comments. If a production hit is truly fixture-like and not a phase transition, move it behind a local helper with a clear name and update the guard allow-list explicitly.

- [ ] **Step 4: Run the guard test and verify it passes**

Run:

```bash
cargo test production_code_does_not_call_with_phase_directly --test phase_transition_usage_tests
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add tests/phase_transition_usage_tests.rs src/api src/worker src/tx
git commit -m "test: guard validated transaction phase changes"
```

---

### Task 6: Documentation And CI Closure

**Files:**
- Modify: `TODOS.md`
- Modify: `README.md`
- Optional Modify: `docs/bug-inventory-current-2026-05-30.md`

- [ ] **Step 1: Update `TODOS.md` Phase 1 status**

After implementation and CI pass, update the Phase 1 section to say:

```markdown
**状态**：已完成并通过 GitHub Actions。生产事务路径通过 `TxJournalRecord::transition_phase()` 进行 phase 变更；`with_phase()` 仍保留给测试 fixture 和后续 public-field 封装迁移。
```

- [ ] **Step 2: Update README development order only if Phase 1 is complete**

Replace README line in the development order:

```text
1. 保持事务正确性优先：当前先完成 Phase 1 状态机重构，新增或迁移 phase 写入必须补 recovery/journal/shadow 回归测试。
```

with:

```text
1. 保持事务正确性优先：状态机重构已完成；后续新增 phase 写入必须走 transition_phase 并补 recovery/journal/shadow 回归测试。
```

- [ ] **Step 3: Run formatting and tests**

Run locally if available:

```bash
cargo fmt --check
cargo test
git diff --check
```

Expected: all pass.

If local `cargo` is unavailable:

```bash
git diff --check
git push origin HEAD:<branch-name>
gh run watch
```

Expected: GitHub Actions passes Rust `cargo check`, Rust `cargo test`, Python adapter, real-device probe build, and fake-adapter integration matrix.

- [ ] **Step 4: Commit docs**

```bash
git add README.md TODOS.md docs/bug-inventory-current-2026-05-30.md
git commit -m "docs: mark transaction phase state machine complete"
```

Only include `docs/bug-inventory-current-2026-05-30.md` if it was actually updated.

---

## Self-Review

- Spec coverage: This plan covers the corrected Phase 1 scope from `TODOS.md` and the 2026-05-30 structural refactor review. It explicitly excludes Phase 2/3.
- Placeholder scan: No `TBD`, `TODO`, or "fill in later" instructions remain.
- Type consistency: The plan consistently uses `TxJournalRecord::transition_phase(TxPhase) -> UnderlayResult<()>`, `validate_transition(&TxPhase, &TxPhase)`, and `UnderlayError::InvalidPhaseTransition { from, to }`.
- Risk note: Local `cargo` is unavailable on this machine, so Rust validation must be closed through GitHub Actions.
