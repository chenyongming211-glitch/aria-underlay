# BGP Policy Dependency Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add BGP route-policy dependency evidence to dry-run and offline reports without enabling any BGP renderer or device write path.

**Architecture:** Rust ChangePlan remains the authoritative dry-run decision layer. BGP neighbor import/export route-policy strings become structured dependency evidence and fail-closed when the dry-run input lacks matching policy evidence. SoT Snapshot records route-policy and prefix-list ownership as an input boundary; Python offline H3C acceptance reports parsed BGP policy dependencies but keeps write decisions read-only/rejected.

**Tech Stack:** Rust domain/dry-run models, serde JSON output, Python offline H3C acceptance runner, GitHub Actions for Rust validation.

---

### Task 1: ChangePlan Route-Policy Dependencies

**Files:**
- Modify: `src/engine/change_plan.rs`
- Modify: `src/engine/dry_run.rs`
- Modify: `src/planner/device_plan.rs`
- Test: `tests/change_plan_tests.rs`

- [ ] Write a failing Rust test that creates a BGP neighbor with `import_policy` and `export_policy`, supplies only one route-policy evidence entry, and expects two dependency edges plus one `bgp: missing route-policy evidence <name>` unsupported path.
- [ ] Add a small route-policy evidence set to `DeviceDesiredState`.
- [ ] Add `RoutePolicyDependency` output to `ChangePlan`.
- [ ] Add `build_change_plan_with_profile_and_route_policy_evidence()` and route dry-run through it.
- [ ] Verify the test fails before implementation and passes after implementation through GitHub CI, because local `cargo` is unavailable.

### Task 2: SoT Route-Policy Boundary

**Files:**
- Modify: `src/sot/snapshot.rs`
- Test: `tests/sot_tests.rs`

- [ ] Write failing SoT tests for route-policy and prefix-list records.
- [ ] Add `SotRoutePolicy` and `SotPrefixList`.
- [ ] Validate duplicate records, unknown devices, source metadata, and route-policy references to prefix-lists or ACLs.

### Task 3: Offline H3C Policy Dependency Report

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/acceptance/offline_h3c.py`
- Test: `adapter-python/tests/test_offline_h3c_acceptance.py`
- Modify: `README.md`
- Modify: `TODOS.md`

- [ ] Write failing Python tests expecting `policy_dependencies` and `missing_policy_refs` in read-only and real-sample BGP audits.
- [ ] Derive dependency entries from parsed BGP neighbor details.
- [ ] Include missing policy refs in JSON and human-readable summaries.
- [ ] Update docs to state that policy dependencies are modeled for dry-run/report only and still do not enable BGP writes.

### Verification

- [ ] Run `python3 -m pytest adapter-python/tests -q`.
- [ ] Run `git diff --check`.
- [ ] Push branch and wait for GitHub CI to pass.
- [ ] After CI is green, fast-forward merge into `main`, push `main`, and wait for main CI to pass.
