# Sprint 2E Validator Usability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve `aria-underlay-state-parse` with pretty JSON and summary output for field XML sample triage.

**Architecture:** Extend `adapter-python/aria_underlay_adapter/state_parsers/validator.py` with output formatting and summary construction. Keep parsing, registry selection, and production readiness gates unchanged.

**Tech Stack:** Python 3.10+, argparse, json, pytest.

---

### Task 1: Pretty Output

**Files:**
- Modify: `adapter-python/tests/test_state_parser_validator.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/validator.py`

- [ ] **Step 1: Write failing pretty output test**

Add a test that calls `validator.main([... "--pretty"])` and asserts stdout contains indented JSON with a newline and remains parseable by `json.loads`.

- [ ] **Step 2: Run test to verify it fails**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py::test_validator_pretty_prints_observed_state_json -q`

Expected: fail because `--pretty` is not recognized.

- [ ] **Step 3: Implement pretty flag**

Add `--pretty` to argparse and print JSON with `indent=2` when enabled.

- [ ] **Step 4: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py::test_validator_pretty_prints_observed_state_json -q`

Expected: pass.

### Task 2: Summary Output

**Files:**
- Modify: `adapter-python/tests/test_state_parser_validator.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/validator.py`

- [ ] **Step 1: Write failing summary test**

Add a test that calls `validator.main([... "--summary"])` and asserts `profile_name`, `fixture_verified`, `production_ready`, `vlan_count`, and `interface_count`.

- [ ] **Step 2: Write scoped summary test**

Add a test that calls `validator.main([... "--summary", "--vlan", "100", "--interface", "GE1/0/1"])` and asserts counts are scoped to 1 and the scope payload includes requested values.

- [ ] **Step 3: Run summary tests to verify failure**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py -q`

Expected: summary tests fail because `--summary` is not implemented.

- [ ] **Step 4: Implement summary payload**

Build summary from `parser.profile`, parsed state, and scope. Keep error JSON unchanged.

- [ ] **Step 5: Run validator tests**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py -q`

Expected: pass.

### Task 3: Docs and Verification

**Files:**
- Modify: `adapter-python/README.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Document field usage**

Update README with `--summary` and `--pretty` examples.

- [ ] **Step 2: Update progress docs**

Add Sprint 2E status and keep the production-readiness boundary explicit.

- [ ] **Step 3: Run full adapter tests**

Run: `python3 -m pytest adapter-python/tests -q`

Expected: all adapter tests pass.

- [ ] **Step 4: Check whitespace and commit**

Run: `git diff --check`

Expected: no whitespace errors.

Commit only Sprint 2E files; do not include unrelated `.gitignore` or `.claude/`.
