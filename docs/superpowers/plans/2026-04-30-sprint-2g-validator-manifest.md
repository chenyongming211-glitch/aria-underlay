# Sprint 2G Validator Manifest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `aria-underlay-state-parse` with JSON manifest batch validation for redacted NETCONF running XML samples.

**Architecture:** Keep the existing single-sample path intact. Add a manifest path in `adapter-python/aria_underlay_adapter/state_parsers/validator.py` that validates manifest shape, resolves relative XML paths from the manifest directory, parses each sample independently, and prints one batch report.

**Tech Stack:** Python 3.10+, argparse, json, pathlib, pytest.

---

### Task 1: Manifest Success Path

**Files:**
- Modify: `adapter-python/tests/test_state_parser_validator.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/validator.py`

- [ ] **Step 1: Write failing test for successful manifest**

Add `test_validator_manifest_outputs_batch_summary_for_successful_samples`. It should create a manifest with Huawei and H3C fixture XML paths, call `validator.main(["--manifest", str(manifest)])`, and assert `ok=True`, `sample_count=2`, `passed=2`, `failed=0`, plus per-sample summaries.

- [ ] **Step 2: Run test to verify failure**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py::test_validator_manifest_outputs_batch_summary_for_successful_samples -q`

Expected: fail because `--manifest` is not recognized.

- [ ] **Step 3: Implement manifest success path**

Add `--manifest`, parse JSON, validate `samples`, resolve XML paths, reuse parser lookup and `_summary`, and print batch JSON.

- [ ] **Step 4: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py::test_validator_manifest_outputs_batch_summary_for_successful_samples -q`

Expected: pass.

### Task 2: Manifest Failure Reporting

**Files:**
- Modify: `adapter-python/tests/test_state_parser_validator.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/validator.py`

- [ ] **Step 1: Write failing mixed-result test**

Add `test_validator_manifest_reports_all_samples_when_one_parse_fails`. It should create one valid XML sample and one invalid XML sample, run manifest validation, assert return code `1`, stdout report `ok=False`, `passed=1`, `failed=1`, and failed sample error code `NETCONF_STATE_PARSE_FAILED`.

- [ ] **Step 2: Write failing invalid-manifest test**

Add `test_validator_manifest_returns_structured_error_for_invalid_shape`. It should write `{"samples": {}}`, run manifest validation, assert return code `1`, stdout empty, stderr JSON with code `STATE_PARSER_MANIFEST_INVALID`.

- [ ] **Step 3: Run tests to verify failure**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py -q`

Expected: new manifest failure tests fail until manifest error handling is implemented.

- [ ] **Step 4: Implement per-sample and manifest errors**

Convert `AdapterError` into result-level `error` objects for sample parser failures. Add a small validator error helper for malformed manifests that writes structured JSON to stderr.

- [ ] **Step 5: Run validator tests**

Run: `python3 -m pytest adapter-python/tests/test_state_parser_validator.py -q`

Expected: pass.

### Task 3: Docs and Verification

**Files:**
- Modify: `adapter-python/README.md`
- Modify: `adapter-python/tests/fixtures/state_parsers/real_samples/README.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Document manifest usage**

Add a manifest example and command to the adapter README.

- [ ] **Step 2: Update real sample fixture policy**

Document optional manifest files beside real samples and the batch validation command.

- [ ] **Step 3: Update progress docs**

Add Sprint 2G status and keep the real-device and production-readiness boundary explicit.

- [ ] **Step 4: Run full adapter tests**

Run: `python3 -m pytest adapter-python/tests -q`

Expected: all adapter tests pass.

- [ ] **Step 5: Check whitespace and commit**

Run: `git diff --check`

Expected: no whitespace errors.

Commit only Sprint 2G files; do not include unrelated `.gitignore` or `.claude/`.
