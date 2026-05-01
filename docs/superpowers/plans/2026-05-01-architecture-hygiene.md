# Architecture Hygiene Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce architecture drift before more real-device features are added.

**Architecture:** Keep the existing Rust Core / Python Adapter split. Make low-risk truth-in-docs and placeholder cleanup first, then do behavior-preserving boundary refactors for Rust service orchestration and Python NETCONF backend internals.

**Tech Stack:** Rust, Python, pytest, GitHub Actions, gRPC/protobuf.

---

### Task 1: Docs Truth Package

**Files:**
- Modify: `README.md`
- Modify: `adapter-python/README.md`
- Modify: `docs/progress-2026-04-26.md`
- Create: `docs/superpowers/plans/2026-05-01-architecture-hygiene.md`

- [ ] **Step 1: Update the public project status**

Replace stale Sprint 0 wording with current status: Rust transaction core is CI-verified, Python NETCONF adapter has fail-closed production gates, Huawei/H3C parser and renderer remain non-production until real-device XML evidence exists, and NAPALM/Netmiko are not implemented.

- [ ] **Step 2: Verify docs-only changes**

Run: `git diff --check`

Expected: exit code 0.

- [ ] **Step 3: Commit and push**

Run:

```bash
git add README.md adapter-python/README.md docs/progress-2026-04-26.md docs/superpowers/plans/2026-05-01-architecture-hygiene.md
git commit -m "docs: refresh architecture truth"
git push origin main
gh run watch <new-run-id> --exit-status
```

Expected: GitHub Actions run exits 0 before Task 2 starts.

### Task 2: Python Placeholder Cleanup Package

**Files:**
- Modify or delete: `adapter-python/aria_underlay_adapter/backends/netmiko_backend.py`
- Modify or delete: `adapter-python/aria_underlay_adapter/backends/napalm_backend.py`
- Modify or delete: `adapter-python/aria_underlay_adapter/diff.py`
- Modify or delete: `adapter-python/aria_underlay_adapter/rollback.py`
- Modify or delete: `adapter-python/aria_underlay_adapter/state.py`
- Modify: `adapter-python/tests/test_driver_registry.py` or add a focused placeholder cleanup test if any public import is preserved.
- Modify: `adapter-python/README.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Confirm imports and choose cleanup shape**

Run: `rg -n "NetmikoBackend|NapalmBackend|DiffEngine|RollbackManager|StateReader|from aria_underlay_adapter\\.diff|from aria_underlay_adapter\\.rollback|from aria_underlay_adapter\\.state" adapter-python src tests docs`

Expected: Only direct placeholder files and docs references appear. If code imports a placeholder, preserve the import with explicit unsupported behavior; otherwise delete the unused placeholder file.

- [ ] **Step 2: Write or update tests**

If preserving public imports, add tests that construction succeeds and operations fail closed with a clear unsupported error. If deleting unused files, rely on the full adapter test suite and import search to prove no consumer remains.

- [ ] **Step 3: Implement cleanup**

Delete unused placeholder modules or convert them to explicit unsupported modules. Do not leave bare `pass` files in production package paths.

- [ ] **Step 4: Verify locally**

Run:

```bash
python3 -m pytest adapter-python/tests
git diff --check
```

Expected: `230 passed` or higher, and diff check exits 0.

- [ ] **Step 5: Commit and push**

Run:

```bash
git add adapter-python docs
git commit -m "fix: remove python placeholder ambiguity"
git push origin main
gh run watch <new-run-id> --exit-status
```

Expected: GitHub Actions run exits 0 before Task 3 starts.

### Task 3: Rust Service Boundary Package

**Files:**
- Modify: `src/api/service.rs`
- Create or modify: `src/api/apply.rs`
- Create or modify: `src/api/recovery_ops.rs`
- Create or modify: `src/api/drift_ops.rs`
- Modify: `src/api/mod.rs`
- Modify: Rust tests only if public paths change.
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Establish behavior baseline**

Run: `cargo test` when Rust is available. In the current local environment, `cargo` is unavailable, so Rust compilation is verified by GitHub Actions.

- [ ] **Step 2: Move apply orchestration without changing behavior**

Extract apply-specific helpers from `src/api/service.rs` into `src/api/apply.rs`. Keep public `UnderlayService` behavior unchanged.

- [ ] **Step 3: Move recovery/admin helper logic without changing behavior**

Extract recovery helpers into `src/api/recovery_ops.rs` and keep force-resolve public API behavior unchanged.

- [ ] **Step 4: Move drift helper logic without changing behavior**

Extract drift audit helper logic into `src/api/drift_ops.rs`, keeping lifecycle transitions and observed-store semantics unchanged.

- [ ] **Step 5: Verify locally where possible**

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests
```

Expected: Python suite passes and diff check exits 0. Rust compile/test result comes from GitHub Actions if local `cargo` is still unavailable.

- [ ] **Step 6: Commit and push**

Run:

```bash
git add src/api docs/progress-2026-04-26.md
git commit -m "refactor: split underlay service boundaries"
git push origin main
gh run watch <new-run-id> --exit-status
```

Expected: GitHub Actions run exits 0 before Task 4 starts.

### Task 4: Python NETCONF Boundary Package

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf.py`
- Create: `adapter-python/aria_underlay_adapter/backends/netconf_hostkey.py`
- Create: `adapter-python/aria_underlay_adapter/backends/netconf_errors.py`
- Create: `adapter-python/aria_underlay_adapter/backends/netconf_state.py`
- Create: `adapter-python/aria_underlay_adapter/backends/netconf_candidate.py`
- Modify: `adapter-python/tests/test_netconf_backend.py`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Write focused import/behavior tests if needed**

Preserve existing tests as the behavioral contract. Add small tests only when a helper becomes directly testable and the existing suite does not cover the extracted branch.

- [ ] **Step 2: Extract host-key helpers**

Move TOFU store, known-hosts path validation, remote-key extraction, session close, and atomic trust-store writes into `netconf_hostkey.py`. Keep `NcclientNetconfBackend._connect()` behavior unchanged.

- [ ] **Step 3: Extract error mapping**

Move `_adapter_error_from_ncclient_exception()` and `_adapter_operation_error()` into `netconf_errors.py`. Keep error codes and retryable flags unchanged.

- [ ] **Step 4: Extract state read/verify helpers**

Move running filter, running reply extraction, parser gate, observed-state shape validation, and verify helpers into `netconf_state.py`.

- [ ] **Step 5: Extract candidate operation helpers if the resulting boundary is clearer**

Move candidate lock, unlock, discard, edit, validate, commit, final confirm, and rollback helpers into `netconf_candidate.py` only if the extracted API remains smaller and more readable than the current class-local methods.

- [ ] **Step 6: Verify locally**

Run:

```bash
python3 -m pytest adapter-python/tests
git diff --check
```

Expected: adapter tests pass and diff check exits 0.

- [ ] **Step 7: Commit and push**

Run:

```bash
git add adapter-python docs/progress-2026-04-26.md
git commit -m "refactor: split netconf backend helpers"
git push origin main
gh run watch <new-run-id> --exit-status
```

Expected: GitHub Actions run exits 0.
