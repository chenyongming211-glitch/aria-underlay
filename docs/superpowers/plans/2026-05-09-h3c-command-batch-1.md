# H3C Command Batch 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete H3C ACL rule description support and implement explicit delete-intent boundaries for isolated VLAN, ACL, and ACL binding targets.

**Architecture:** Keep ACL rule description on the existing `AclRule.description` field. Add explicit delete fields to the domain/device/protobuf contract so merge/upsert never infers deletes from omitted desired objects.

**Tech Stack:** Rust domain/probe code, Python H3C adapter renderer/parser, pytest, GitHub Actions for Rust verification.

---

### Task 1: Document Batch Scope

**Files:**
- Create: `docs/h3c-command-adaptation-roadmap-2026-05-09.md`
- Create: `docs/superpowers/specs/2026-05-09-h3c-command-batch-1-design.md`

- [x] Record the current H3C production command surface.
- [x] Record batch ordering for ACL description, explicit delete intent, ACL family expansion, QoS, PBR, NQA, BGP, and cross-device atomicity.
- [x] Record that delete must be explicit and must not be inferred from missing desired state.

### Task 2: Add Failing ACL Rule Description Tests

**Files:**
- Modify: `adapter-python/tests/test_renderers.py`
- Modify: `adapter-python/tests/test_state_parsers.py`

- [x] Add a renderer assertion that an ACL rule with `description="allow test flow"` emits `IPv4AdvanceRules/Rule/Description`.
- [x] Add a parser fixture assertion that `IPv4AdvanceRules/Rule/Description` is read into each rule's `description`.
- [x] Run the focused pytest selection remotely and confirm the tests fail before implementation.

### Task 3: Implement H3C ACL Rule Description Round Trip

**Files:**
- Modify: `adapter-python/aria_underlay_adapter/renderers/h3c.py`
- Modify: `adapter-python/aria_underlay_adapter/state_parsers/h3c.py`

- [x] Append `Description` to H3C ACL rule XML when the desired rule description is non-empty.
- [x] Parse optional `Description` from H3C ACL rule XML.
- [x] Re-run focused pytest and full adapter pytest remotely.

### Task 4: Wire Real-Device Acceptance Inputs

**Files:**
- Modify: `examples/real_domain_apply_probe.rs`
- Modify: `docs/examples/real-device-acceptance.env.example`
- Modify: `docs/runbooks/real-device-acceptance.md`
- Modify: `docs/runbooks/real-device-acceptance-checklist.md`
- Modify: `docs/runbooks/real-device-acceptance-record-template.md`

- [x] Read `ARIA_UNDERLAY_ACL_RULE_DESCRIPTION` in `real_domain_apply_probe`.
- [x] Document the environment variable and readback requirement.
- [ ] Use GitHub Actions to verify Rust compilation and tests after pushing.

### Task 5: Implement Explicit Delete Intent

**Files:**
- Modify: `src/intent/domain.rs`
- Modify: `src/intent/validation.rs`
- Modify: `src/planner/device_plan.rs`
- Modify: `src/planner/domain_plan.rs`
- Modify: `src/engine/diff.rs`
- Modify: `src/engine/dry_run.rs`
- Modify: `src/adapter_client/mapper.rs`
- Modify: `proto/aria_underlay_adapter.proto`
- Modify: `adapter-python/aria_underlay_adapter/renderers/h3c.py`
- Modify: `adapter-python/aria_underlay_adapter/backends/netconf_state.py`
- Modify: `adapter-python/aria_underlay_adapter/backends/mock_netconf.py`

- [x] Add explicit delete fields for VLAN ids, ACL ids, and ACL bindings.
- [x] Keep merge/upsert from inferring delete from absence.
- [x] Preserve full-replace delete semantics.
- [x] Render H3C NETCONF delete XML for supported explicit delete targets.
- [x] Verify delete targets are absent after scoped readback.
- [x] Update shadow state from the actual change set so merge/upsert does not drop unrelated managed state.
- [ ] Run Rust and Python verification after protobuf regeneration.
