# Sprint 2F NETCONF State Sample Runbook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Document the safe field workflow for capturing, redacting, validating, and turning real NETCONF running XML into parser fixtures.

**Architecture:** Add a runbook under `docs/runbooks/`, add a real sample fixture README under `adapter-python/tests/fixtures/state_parsers/real_samples/`, and update the progress report. This is a documentation-only change and does not modify runtime parser behavior.

**Tech Stack:** Markdown, existing `aria-underlay-state-parse` CLI.

---

### Task 1: Capture Runbook

**Files:**
- Create: `docs/runbooks/netconf-state-sample-capture.md`

- [ ] **Step 1: Write capture prerequisites**

Document required lab access, NETCONF connectivity, vendor/model/OS metadata, and the rule that raw XML stays outside git.

- [ ] **Step 2: Write capture commands**

Document a repeatable `ncclient` capture snippet and the validator commands:

```bash
aria-underlay-state-parse --vendor huawei --xml sample.xml --summary
aria-underlay-state-parse --vendor huawei --xml sample.xml --pretty
```

- [ ] **Step 3: Write redaction checklist**

Document exact categories that must be redacted before any sample enters the repo.

### Task 2: Real Sample Fixture Policy

**Files:**
- Create: `adapter-python/tests/fixtures/state_parsers/real_samples/README.md`

- [ ] **Step 1: Define directory layout**

Document `real_samples/<vendor>/<profile>/<sample-name>.redacted.xml`.

- [ ] **Step 2: Define metadata requirements**

Document required metadata: vendor, model, OS version, capture source, redaction notes, validator command, validator result.

- [ ] **Step 3: Define promotion rule**

Document that a successful sample does not automatically set `production_ready=True`.

### Task 3: Progress and Verification

**Files:**
- Modify: `docs/progress-2026-04-26.md`

- [ ] **Step 1: Add Sprint 2F progress section**

Record that sample capture and fixture policy are documented, but real samples are still missing.

- [ ] **Step 2: Run markdown sanity checks**

Run: `git diff --check`

Expected: no whitespace errors.

- [ ] **Step 3: Commit**

Commit only Sprint 2F docs and leave unrelated `.gitignore` / `.claude/` untouched.
