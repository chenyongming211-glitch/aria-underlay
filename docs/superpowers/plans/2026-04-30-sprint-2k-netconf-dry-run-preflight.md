# Sprint 2K NETCONF Dry-Run Preflight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Python `NetconfBackedDriver.dry_run()` `NotImplementedError` with a long-term fail-closed preflight path.

**Architecture:** Introduce a backend-level `dry_run_candidate()` contract that reuses the same renderer production gate and candidate XML rendering validation used by prepare. Real NETCONF dry-run remains read-only and does not open a device session; mock NETCONF dry-run simulates desired-vs-running changes without mutating running state.

**Tech Stack:** Python adapter, protobuf `DryRunResponse`, renderer registry, pytest.

---

### Task 1: Backend Dry-Run Contract

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/backends/base.py`
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf.py`
- Test: `adapter-python/tests/test_netconf_backend.py`

- [x] Add `CandidateDryRunResult` to the backend protocol.
- [x] Add `NcclientNetconfBackend.dry_run_candidate()`.
- [x] Extract shared renderer validation into `_render_candidate_config()`.
- [x] Keep `dry_run_candidate()` read-only: no NETCONF connect, no lock, no edit-config.

### Task 2: Driver Integration

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py`
- Test: `adapter-python/tests/test_netconf_backend.py`

- [x] Implement `NetconfBackedDriver.dry_run()`.
- [x] Reuse renderer registry selection for non-empty desired state.
- [x] Fail closed for unsupported vendors and skeleton renderers.
- [x] Return structured `DryRunResponse` errors instead of raising `NotImplementedError`.

### Task 3: Mock Backend Coverage

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/backends/mock_netconf.py`
- Test: `adapter-python/tests/test_fake_driver.py`

- [x] Add mock `dry_run_candidate()` that previews `_merge_desired_state()` without mutating running state.
- [x] Preserve `validate_failed` dry-run failure behavior in mock profiles.
- [x] Update fake driver service tests so dry-run is a real operation.

### Task 4: Verification

**Files:**
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/aria-underlay-development-plan.md`

- [x] Record the long-term-stability-first development principle.
- [x] Document dry-run preflight behavior and scope limits.
- [x] Run targeted dry-run tests.
- [x] Run full Python adapter tests.
- [x] Run `git diff --check`.
