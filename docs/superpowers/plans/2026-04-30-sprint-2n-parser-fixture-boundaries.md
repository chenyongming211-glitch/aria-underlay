# Parser Fixture Boundaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand Huawei/H3C state parser fixture coverage so parser boundaries fail closed for malformed or unsupported XML without requiring real switches.

**Architecture:** Keep the existing fixture parser architecture and add file-backed positive/negative XML fixtures. Tests load fixtures by vendor, assert expected parsed state for namespace variants, and assert structured `NETCONF_STATE_PARSE_FAILED` errors for invalid samples.

**Tech Stack:** Python, pytest, `xml.etree.ElementTree`, existing `FixtureStateParser`.

---

### Task 1: Positive Namespace Fixtures

**Files:**
- Create: `adapter-python/tests/fixtures/state_parsers/huawei/vrp8_namespaced.xml`
- Create: `adapter-python/tests/fixtures/state_parsers/h3c/comware7_namespaced.xml`
- Modify: `adapter-python/tests/test_state_parsers.py`

- [ ] Add namespace-qualified running XML samples with VLAN and interface data.
- [ ] Add tests proving Huawei and H3C parsers ignore XML namespace prefixes via local-name matching.
- [ ] Run `cd adapter-python && pytest tests/test_state_parsers.py -q`.

### Task 2: Negative Boundary Fixtures

**Files:**
- Create XML files under `adapter-python/tests/fixtures/state_parsers/negative/huawei/`
- Create XML files under `adapter-python/tests/fixtures/state_parsers/negative/h3c/`
- Modify: `adapter-python/tests/test_state_parsers.py`

- [ ] Add missing VLAN ID, duplicate VLAN, invalid VLAN, duplicate interface, empty interface name, unknown mode, trunk without VLANs, and duplicate allowed VLAN fixtures.
- [ ] Add a table-driven pytest case that loads each fixture and asserts `NETCONF_STATE_PARSE_FAILED` plus the expected raw summary.
- [ ] Run `cd adapter-python && pytest tests/test_state_parsers.py -q`.

### Task 3: Parser Hardening and Docs

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/common.py` if tests expose parser gaps.
- Modify: `docs/progress-2026-04-26.md`

- [ ] Tighten parser behavior only where tests expose a real gap.
- [ ] Record Sprint 2N fixture coverage and the fact that parsers remain fixture-verified, not production-ready.
- [ ] Run `cd adapter-python && pytest -q`.
- [ ] Run `git diff --check`.
