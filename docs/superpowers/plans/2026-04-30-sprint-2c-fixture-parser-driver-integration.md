# Sprint 2C Fixture Parser Driver Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire fixture-verified Huawei/H3C state parsers through the NETCONF-backed driver in tests while preserving production fail-closed parser selection.

**Architecture:** Add a driver-level opt-in fixture parser gate that is disabled by default. Use existing NETCONF backend session fakes and XML fixtures to exercise driver `GetCurrentState` and `Verify` through real parser, scope, and diff logic.

**Tech Stack:** Python 3.10+, pytest, existing NETCONF backend fake session helpers, protobuf-generated adapter messages.

---

### Task 1: Driver Fixture Parser Gate

**Files:**
- Modify: `adapter-python/tests/test_netconf_backend.py`
- Modify: `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py`

- [ ] **Step 1: Write failing opt-in driver state test**

Add a test that constructs `NetconfBackedDriver(_BackendWithSession(session), allow_fixture_verified_parser=True)`, reads the Huawei fixture through `_Reply`, and asserts `GetCurrentState` contains VLAN 100 and interface `GE1/0/1`.

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest adapter-python/tests/test_netconf_backend.py::test_netconf_driver_get_state_can_use_fixture_verified_parser_when_enabled -q`

Expected: fail because `NetconfBackedDriver.__init__()` does not accept `allow_fixture_verified_parser`.

- [ ] **Step 3: Implement minimal constructor flag**

Store `allow_fixture_verified_parser=False` in `NetconfBackedDriver`, and pass it to `state_parser_for_vendor()` inside `_backend_for_state_read()`.

- [ ] **Step 4: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_netconf_backend.py::test_netconf_driver_get_state_can_use_fixture_verified_parser_when_enabled -q`

Expected: pass.

### Task 2: Driver Verify Integration

**Files:**
- Modify: `adapter-python/tests/test_netconf_backend.py`

- [ ] **Step 1: Write matching verify test**

Add a driver-level `verify()` test using the Huawei fixture and matching `pb2.DesiredDeviceState`; assert response status is `ADAPTER_OPERATION_STATUS_NO_CHANGE`.

- [ ] **Step 2: Run test to verify it fails before Task 1 implementation or passes after Task 1**

Run: `python3 -m pytest adapter-python/tests/test_netconf_backend.py::test_netconf_driver_verify_succeeds_with_fixture_verified_parser_when_enabled -q`

Expected: pass only after the opt-in driver parser gate exists.

- [ ] **Step 3: Write mismatch verify test**

Add a driver-level `verify()` test where desired VLAN 100 has the wrong name; assert response status is failed and error code is `VERIFY_FAILED`.

- [ ] **Step 4: Run verify tests**

Run: `python3 -m pytest adapter-python/tests/test_netconf_backend.py -q`

Expected: pass.

### Task 3: Scope Integration

**Files:**
- Modify: `adapter-python/tests/test_netconf_backend.py`

- [ ] **Step 1: Write scoped state test**

Add a driver-level `GetCurrentState` test with `scope=StateScope(vlan_ids=[100], interface_names=["GE1/0/1"])`; assert only VLAN 100 and interface `GE1/0/1` are returned and the session used a scoped subtree filter.

- [ ] **Step 2: Write empty scope driver test**

Add a driver-level `GetCurrentState` test with `StateScope(full=False)`; assert an empty observed state and no session calls.

- [ ] **Step 3: Run scoped tests**

Run: `python3 -m pytest adapter-python/tests/test_netconf_backend.py -q`

Expected: pass.

### Task 4: Docs and Full Adapter Verification

**Files:**
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Update progress docs**

Add a Sprint 2C section that states fixture parser driver/backend integration is locally verified and production readiness is still blocked on real device XML.

- [ ] **Step 2: Run adapter tests**

Run: `python3 -m pytest adapter-python/tests -q`

Expected: all adapter tests pass.

- [ ] **Step 3: Check whitespace and commit**

Run: `git diff --check`

Expected: no whitespace errors.

Commit only Sprint 2C files and do not include unrelated `.gitignore` or `.claude/` worktree state.
