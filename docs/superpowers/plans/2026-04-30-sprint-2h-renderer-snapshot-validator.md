# Sprint 2H Renderer Snapshot Validator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an offline `aria-underlay-render-snapshot` command that renders desired-state JSON through Huawei/H3C skeleton renderers and emits a stable JSON snapshot report.

**Architecture:** Add `adapter-python/aria_underlay_adapter/renderers/snapshot.py` as the CLI entrypoint. Keep production renderer registry behavior unchanged by using `renderer_for_vendor(..., allow_skeleton=True)` only inside this offline tool.

**Tech Stack:** Python 3.10+, argparse, json, types.SimpleNamespace, pytest.

---

### Task 1: Successful Snapshot Output

**Files:**
- Create: `adapter-python/aria_underlay_adapter/renderers/snapshot.py`
- Modify: `adapter-python/tests/test_renderer_snapshot.py`
- Modify: `adapter-python/pyproject.toml`

- [ ] **Step 1: Write failing CLI success test**

Add a test that writes desired-state JSON with one VLAN and one access interface, calls `snapshot.main(["--vendor", "huawei", "--desired-state", str(path)])`, and asserts stdout JSON includes `vendor`, `profile_name`, `production_ready=False`, counts, and XML containing VLAN/interface elements.

- [ ] **Step 2: Run test to verify failure**

Run: `python3 -m pytest adapter-python/tests/test_renderer_snapshot.py::test_render_snapshot_outputs_xml_report_for_huawei -q`

Expected: fail because `snapshot.py` does not exist.

- [ ] **Step 3: Implement minimal snapshot CLI**

Implement argparse, JSON loading, conversion to `SimpleNamespace`, skeleton renderer selection, XML rendering, and JSON output.

- [ ] **Step 4: Run focused test**

Run: `python3 -m pytest adapter-python/tests/test_renderer_snapshot.py::test_render_snapshot_outputs_xml_report_for_huawei -q`

Expected: pass.

### Task 2: Pretty Output and Fail-Closed Errors

**Files:**
- Modify: `adapter-python/tests/test_renderer_snapshot.py`
- Modify: `adapter-python/aria_underlay_adapter/renderers/snapshot.py`

- [ ] **Step 1: Write pretty output test**

Add a test that passes `--pretty` and asserts stdout starts with `{\n` and remains parseable JSON.

- [ ] **Step 2: Write renderer validation error test**

Add a test with VLAN ID `4095`, assert return code `1`, stdout empty, stderr JSON code `RENDER_SNAPSHOT_FAILED`, and raw summary mentions `range 1..4094`.

- [ ] **Step 3: Write invalid JSON shape test**

Add a test with a JSON array payload, assert return code `1`, stdout empty, stderr JSON code `RENDER_SNAPSHOT_INPUT_INVALID`.

- [ ] **Step 4: Run snapshot tests to verify failure**

Run: `python3 -m pytest adapter-python/tests/test_renderer_snapshot.py -q`

Expected: new tests fail until error mapping is complete.

- [ ] **Step 5: Implement errors**

Map malformed input and renderer `ValueError` / `AdapterError` to structured JSON stderr.

- [ ] **Step 6: Run snapshot tests**

Run: `python3 -m pytest adapter-python/tests/test_renderer_snapshot.py -q`

Expected: pass.

### Task 3: Docs and Verification

**Files:**
- Modify: `adapter-python/README.md`
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Document renderer snapshot usage**

Add an offline renderer snapshot section with desired-state JSON and command examples.

- [ ] **Step 2: Update progress docs**

Add Sprint 2H status and keep skeleton production-readiness boundaries explicit.

- [ ] **Step 3: Run full adapter tests**

Run: `python3 -m pytest adapter-python/tests -q`

Expected: all adapter tests pass.

- [ ] **Step 4: Check whitespace and commit**

Run: `git diff --check`

Expected: no whitespace errors.

Commit only Sprint 2H files; do not include unrelated `.gitignore` or `.claude/`.
