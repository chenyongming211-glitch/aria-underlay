# Sprint 2D State Parser Validator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an offline Python CLI that validates captured NETCONF running XML with fixture-verified state parsers and emits normalized observed-state JSON.

**Architecture:** Implement a small `state_parsers/validator.py` module with an argparse-based `main(argv)` function, expose it as a console script in `adapter-python/pyproject.toml`, and test it by calling `main(argv)` with pytest `capsys`.

**Tech Stack:** Python 3.10+, argparse, json, pytest, existing `AdapterError` and state parser registry.

---

### Task 1: Validator CLI Success Path

**Files:**
- Create: `adapter-python/tests/test_state_parser_validator.py`
- Create: `adapter-python/aria_underlay_adapter/state_parsers/validator.py`
- Modify: `adapter-python/pyproject.toml`

- [ ] **Step 1: Write failing Huawei fixture CLI test**

Create a test that calls `validator.main(["--vendor", "huawei", "--xml", "<fixture path>"])`, captures stdout, parses JSON, and asserts VLAN 100 plus interface `GE1/0/1`.

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py::test_validator_outputs_observed_state_json_for_huawei_fixture -q`

Expected: fail because `state_parsers.validator` does not exist.

- [ ] **Step 3: Implement minimal CLI**

Add `main(argv=None)` that parses `--vendor` and `--xml`, reads the file, selects `state_parser_for_vendor(vendor, allow_fixture_verified=True)`, parses XML, prints `json.dumps(state, sort_keys=True)`, and returns `0`.

- [ ] **Step 4: Add console script**

Add `[project.scripts] aria-underlay-state-parse = "aria_underlay_adapter.state_parsers.validator:main"` to `pyproject.toml`.

- [ ] **Step 5: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py::test_validator_outputs_observed_state_json_for_huawei_fixture -q`

Expected: pass.

### Task 2: Scope and Fail-Closed CLI Coverage

**Files:**
- Modify: `adapter-python/tests/test_state_parser_validator.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/validator.py`

- [ ] **Step 1: Write scoped output test**

Call `main()` with `--vlan 100 --interface GE1/0/1` and assert only scoped state is printed.

- [ ] **Step 2: Write unsupported vendor test**

Call `main()` with `--vendor unknown` and assert it returns `1`, stderr JSON has code `STATE_PARSER_VENDOR_UNSUPPORTED`, and stdout is empty.

- [ ] **Step 3: Write invalid XML test**

Use a temporary XML file missing `vlan-id`; assert return code `1` and stderr JSON has code `NETCONF_STATE_PARSE_FAILED`.

- [ ] **Step 4: Implement scope/error handling**

Build a simple scope object from CLI args. Catch `AdapterError`, print JSON error to stderr, and return `1`.

- [ ] **Step 5: Run validator tests**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py -q`

Expected: pass.

### Task 3: Documentation and Full Verification

**Files:**
- Modify: `adapter-python/README.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Document CLI usage**

Add a README section showing how to parse a captured XML sample and scope by VLAN/interface.

- [ ] **Step 2: Update progress docs**

Add Sprint 2D status and keep the production boundary explicit.

- [ ] **Step 3: Run adapter tests**

Run: `python3 -m pytest adapter-python/tests -q`

Expected: all adapter tests pass.

- [ ] **Step 4: Check whitespace and commit**

Run: `git diff --check`

Expected: no whitespace errors.

Commit only Sprint 2D files; do not include unrelated `.gitignore` or `.claude/`.
