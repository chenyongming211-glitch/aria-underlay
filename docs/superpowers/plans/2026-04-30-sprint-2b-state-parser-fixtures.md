# Sprint 2B State Parser Fixtures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add fixture-verified Huawei/H3C NETCONF running-state parsers for VLAN and interface state while keeping production selection fail-closed.

**Architecture:** Add XML fixtures under adapter Python tests, implement shared parser helpers in `state_parsers/common.py`, and keep vendor parser classes thin profile wrappers. Registry default behavior remains fail-closed unless tests explicitly request fixture-verified parsers.

**Tech Stack:** Python 3.10+, `xml.etree.ElementTree`, pytest, existing `AdapterError` and state parser registry.

---

### Task 1: Huawei Fixture Parser

**Files:**
- Create: `adapter-python/tests/fixtures/state_parsers/huawei/vrp8_running.xml`
- Create: `adapter-python/tests/test_state_parsers.py`
- Create: `adapter-python/aria_underlay_adapter/state_parsers/common.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/huawei.py`

- [ ] **Step 1: Write failing Huawei parser test**

Add a test that loads `vrp8_running.xml`, calls `HuaweiStateParser().parse_running(xml)`, and asserts VLAN 100 plus two interfaces: one access port and one trunk port.

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest adapter-python/tests/test_state_parsers.py::test_huawei_parser_reads_fixture_vlan_and_interfaces -q`

Expected: failure because `HuaweiStateParser` still raises `NETCONF_STATE_PARSER_NOT_IMPLEMENTED`.

- [ ] **Step 3: Implement minimal shared XML parsing**

Create `common.py` with profile and helper functions for required text extraction, VLAN validation, duplicate detection, and port mode parsing.

- [ ] **Step 4: Implement Huawei parser wrapper**

Make `HuaweiStateParser` use the shared parser with Huawei fixture namespaces, set `fixture_verified=True`, and keep `production_ready=False`.

- [ ] **Step 5: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_state_parsers.py::test_huawei_parser_reads_fixture_vlan_and_interfaces -q`

Expected: pass.
### Task 2: H3C Fixture Parser

**Files:**
- Create: `adapter-python/tests/fixtures/state_parsers/h3c/comware7_running.xml`
- Modify: `adapter-python/tests/test_state_parsers.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/h3c.py`

- [ ] **Step 1: Write failing H3C parser test**

Add a test that loads `comware7_running.xml`, calls `H3cStateParser().parse_running(xml)`, and asserts the same normalized output shape.

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest adapter-python/tests/test_state_parsers.py::test_h3c_parser_reads_fixture_vlan_and_interfaces -q`

Expected: failure because `H3cStateParser` still raises `NETCONF_STATE_PARSER_NOT_IMPLEMENTED`.

- [ ] **Step 3: Implement H3C parser wrapper**

Use the shared parser with H3C fixture namespaces, set `fixture_verified=True`, and keep `production_ready=False`.

- [ ] **Step 4: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_state_parsers.py::test_h3c_parser_reads_fixture_vlan_and_interfaces -q`

Expected: pass.

### Task 3: Fail-Closed Parser Coverage

**Files:**
- Modify: `adapter-python/tests/test_state_parsers.py`

- [ ] **Step 1: Write invalid XML and duplicate tests**

Add tests for missing VLAN ID, invalid VLAN 4095, duplicate interface names, and unknown port mode.

- [ ] **Step 2: Run tests to verify failures before implementation if needed**

Run: `python3 -m pytest adapter-python/tests/test_state_parsers.py -q`

Expected: any missing validation test fails until helper logic handles it.

- [ ] **Step 3: Tighten helper validation**

Ensure every invalid case raises `AdapterError` with code `NETCONF_STATE_PARSE_FAILED`.

- [ ] **Step 4: Run parser tests**

Run: `python3 -m pytest adapter-python/tests/test_state_parsers.py -q`

Expected: pass.

### Task 4: Registry Fixture Verification Gate

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/registry.py`
- Modify: `adapter-python/tests/test_state_parser_registry.py`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Write registry tests**

Add tests showing default registry selection still rejects Huawei/H3C, while `allow_fixture_verified=True` returns fixture-verified parsers.

- [ ] **Step 2: Run registry tests to verify failure**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_registry.py -q`

Expected: failure because `allow_fixture_verified` does not exist.

- [ ] **Step 3: Implement registry gate**

Add `allow_fixture_verified=False` to `state_parser_for_vendor()`. If parser is not production-ready but is fixture-verified and the flag is true, return it.

- [ ] **Step 4: Run focused Python tests**

Run:

```bash
python3 -m pytest adapter-python/tests/test_state_parsers.py adapter-python/tests/test_state_parser_registry.py -q
```

Expected: pass.
