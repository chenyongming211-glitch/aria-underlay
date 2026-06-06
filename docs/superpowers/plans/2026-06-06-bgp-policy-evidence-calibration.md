# BGP Policy Evidence Calibration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Teach the offline H3C PBR/BGP calibration report to distinguish referenced route-policies that have evidence but remain read-only from route-policies that are missing entirely.

**Architecture:** Keep BGP write paths closed. Add a lightweight evidence resolver in the Python offline H3C acceptance runner, fed by built-in offline fixture evidence, parsed ACL state, and optional real-sample sidecar JSON files. Report dependency status for route-policy, prefix-list, and ACL evidence without enabling renderer or commit behavior.

**Tech Stack:** Python offline acceptance runner and pytest for local verification; Rust remains unchanged for this batch and is validated by GitHub CI because local `cargo` is unavailable.

---

### Task 1: Add Red Tests For Evidence-Aware Reports

**Files:**
- Modify: `adapter-python/tests/test_offline_h3c_acceptance.py`

- [ ] **Step 1: Write failing test for built-in read-only audit evidence**

Add expectations that the built-in PBR/BGP audit reports route-policy evidence as present, leaves `missing_route_policy_refs` empty, and still rejects BGP writes with `bgp: no path-level write evidence`.

- [ ] **Step 2: Write failing test for real sample sidecar evidence**

Add a `sample.redacted.evidence.json` next to the temporary sample XML. The sidecar declares one route-policy with prefix-list and ACL evidence, while the export policy remains undeclared. Expect the report to mark the import policy as `present_read_only`, the export policy as missing, and preserve BGP read-only rejection.

- [ ] **Step 3: Verify RED**

Run:

```bash
python3 -m pytest adapter-python/tests/test_offline_h3c_acceptance.py::test_offline_h3c_acceptance_reports_pbr_bgp_read_only_audit adapter-python/tests/test_offline_h3c_acceptance.py::test_offline_h3c_acceptance_loads_pbr_bgp_real_samples -q
```

Expected: FAIL because the report does not yet include evidence status fields and still treats every route-policy dependency as missing.

### Task 2: Implement Evidence Resolver

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/acceptance/offline_h3c.py`

- [ ] **Step 1: Add evidence loading helpers**

Implement helpers for:

- Built-in offline evidence.
- Optional `<sample>.evidence.json` sidecar loading.
- Parsed ACL evidence extraction from parsed sample state.
- Normalized route-policy, prefix-list, and ACL evidence sets.

- [ ] **Step 2: Add dependency calibration**

Use the resolver to produce:

- `route_policy_dependency_status`
- `route_policy_evidence`
- `missing_route_policy_refs`
- `missing_prefix_list_refs`
- `missing_acl_refs`

Only append `bgp: missing route-policy evidence <name>` when route-policy evidence is absent. Keep `bgp: no path-level write evidence` present whenever BGP is detected.

- [ ] **Step 3: Verify GREEN**

Run the same targeted pytest command. Expected: PASS.

### Task 3: Update Documentation

**Files:**
- Modify: `docs/runbooks/offline-h3c-acceptance.md`
- Modify: `adapter-python/tests/fixtures/state_parsers/real_samples/README.md`
- Modify: `README.md`
- Modify: `TODOS.md`

- [ ] **Step 1: Document sidecar evidence schema**

Document `<sample>.evidence.json` with route-policy, prefix-list, and ACL fields.

- [ ] **Step 2: Document safety semantics**

State that evidence removes “policy missing” findings only. It does not allow BGP writes; write remains rejected/read-only until DeviceModelProfile has path-level write evidence and a renderer exists.

### Task 4: Verify, Commit, Push, CI

**Files:**
- All changed files

- [ ] **Step 1: Run local Python tests**

Run:

```bash
python3 -m pytest adapter-python/tests -q
```

Expected: all Python adapter tests pass.

- [ ] **Step 2: Run whitespace check**

Run:

```bash
git diff --check
```

Expected: no output and exit code 0.

- [ ] **Step 3: Confirm local Rust toolchain availability**

Run:

```bash
command -v cargo; command -v rustfmt
```

Expected: absent locally; use GitHub CI for Rust.

- [ ] **Step 4: Commit and push**

Commit message:

```bash
feat: calibrate bgp policy evidence reports
```

- [ ] **Step 5: Wait for GitHub CI**

Push the branch and wait for the workflow run for the pushed head SHA. If green, merge promptly into `main` per the current project workflow.
