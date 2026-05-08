# Real Device Acceptance Runbook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the ad hoc H3C real-switch validation flow into repeatable documentation, environment examples, and a cleanup utility.

**Architecture:** Keep the real write path in the existing Rust `real_domain_apply_probe`. Add a small Python NETCONF cleanup helper for restoring access/trunk/VLAN test changes, and document the full acceptance sequence in `docs/runbooks`.

**Tech Stack:** Markdown runbooks, Python 3 standard library for payload construction, optional `ncclient` execution on the control node, pytest for cleanup helper unit tests.

---

### Task 1: Cleanup Helper Tests

**Files:**
- Create: `adapter-python/tests/test_real_device_cleanup_script.py`

- [x] Write tests that import `scripts/real_device_cleanup.py` by path.
- [x] Assert access cleanup XML uses `AccessInterfaces`, `IfIndex`, and `PVID`.
- [x] Assert trunk cleanup XML uses `TrunkInterfaces` and `PermitVlanList`.
- [x] Assert VLAN delete XML carries a NETCONF `operation="delete"` attribute.
- [x] Assert the safety gate rejects execution without `--yes` unless `--dry-run` is set.

### Task 2: Cleanup Helper Implementation

**Files:**
- Create: `scripts/real_device_cleanup.py`

- [x] Implement interface-name to IfIndex parsing for H3C long and short aliases.
- [x] Implement XML payload builders for access PVID restore, trunk allowed VLAN restore, and VLAN delete.
- [x] Implement CLI parsing with `--dry-run` and `--yes` safety gates.
- [x] Import `ncclient` and `LocalSecretProvider` only in the execution path.

### Task 3: Acceptance Runbook

**Files:**
- Create: `docs/runbooks/real-device-acceptance.md`
- Create: `docs/runbooks/real-device-acceptance-checklist.md`
- Create: `docs/runbooks/real-device-acceptance-record-template.md`
- Create: `docs/examples/real-device-acceptance.env.example`

- [x] Document prerequisites, device resource selection, write-before checks, dry-run delete guard, apply, readback, cleanup, and failure handling.
- [x] Add a checklist that operators can copy for each device/model.
- [x] Add a record template for commit SHA, adapter image, probe artifact, device resources, tx_id, and cleanup result.
- [x] Add an env example for both access and trunk acceptance runs without secrets.

### Task 4: Verification and Commit

**Files:**
- Modify files from Tasks 1-3.

- [x] Run the focused pytest file for the cleanup helper.
- [x] Run `git diff --check`.
- [x] Review the docs for placeholders and accidental secrets.
- [ ] Commit and push the finished runbook package.
