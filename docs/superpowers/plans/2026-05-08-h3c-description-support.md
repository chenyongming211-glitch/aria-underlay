# H3C Description Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add production H3C VLAN and interface description support using the existing desired-state fields.

**Architecture:** Keep the Rust/proto schema unchanged. Extend the Python H3C renderer to emit Comware `Description` XML, extend the H3C scoped state filter to include `Ifmgr`, and update real-device probe/runbook/cleanup utilities so the new command surface can be tested and restored safely.

**Tech Stack:** Rust examples, Python adapter renderer/parser/filter, pytest, GitHub Actions.

---

### Task 1: Renderer and Filter Tests

**Files:**
- Modify: `adapter-python/tests/test_renderers.py`
- Modify: `adapter-python/tests/test_netconf_backend.py`

- [ ] Add a failing H3C renderer test asserting `render_vlan_create` accepts `description="tenant vlan"` and emits `<Description>tenant vlan</Description>`.
- [ ] Add a failing H3C renderer test asserting `render_edit_config` emits an `Ifmgr/Interfaces/Interface` block with `IfIndex` and `Description` when an interface description is present.
- [ ] Add a failing filter test asserting `build_state_filter(scope, parser=H3cStateParser())` includes both `<Ifmgr>` and `<VLAN>`.
- [ ] Run the focused pytest selection remotely and confirm these tests fail for the expected missing H3C description support.

### Task 2: Renderer and Filter Implementation

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/renderers/h3c.py`
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf_state.py`

- [ ] Update `H3cRenderer.render_vlan_create` to append `Description` when the desired VLAN description is non-empty.
- [ ] Add an H3C interface description renderer that emits `Ifmgr/Interfaces/Interface/IfIndex/Description`.
- [ ] Update `render_edit_config` to include `Ifmgr` and `VLAN` as sibling blocks under the same H3C `top`.
- [ ] Keep the existing access/trunk port-mode XML unchanged.
- [ ] Update H3C scoped state filters to request both `Ifmgr` and `VLAN`.
- [ ] Run the focused pytest selection remotely and confirm it passes.

### Task 3: Cleanup and Real Probe Tests

**Files:**
- Modify: `adapter-python/tests/test_real_device_cleanup_script.py`
- Modify: `scripts/real_device_cleanup.py`
- Modify: `examples/real_domain_apply_probe.rs`

- [ ] Add a failing cleanup script test asserting interface description restore XML includes `Ifmgr`, `IfIndex`, and `Description`.
- [ ] Add a failing cleanup script test asserting `--clear-description` emits a NETCONF delete operation on `Description`.
- [ ] Implement cleanup CLI arguments `--description-interface`, `--description`, and `--clear-description`.
- [ ] Update the real probe to read `ARIA_UNDERLAY_TEST_VLAN_DESCRIPTION`, `ARIA_UNDERLAY_ACCESS_DESCRIPTION`, and `ARIA_UNDERLAY_TRUNK_DESCRIPTION`.
- [ ] Run focused Python tests remotely.

### Task 4: Runbook Updates and Verification

**Files:**
- Modify: `docs/runbooks/real-device-acceptance.md`
- Modify: `docs/runbooks/real-device-acceptance-checklist.md`
- Modify: `docs/runbooks/real-device-acceptance-record-template.md`
- Modify: `docs/examples/real-device-acceptance.env.example`

- [ ] Document the optional description acceptance variables.
- [ ] Document description readback and cleanup checks.
- [ ] Keep the warning that admin-state, native VLAN, and delete semantics are out of scope.
- [ ] Run full adapter Python tests remotely.
- [ ] Run `git diff --check`.
- [ ] Scan touched files for accidental secrets or real lab credentials.
- [ ] Commit, push, and confirm GitHub Actions for the branch.
