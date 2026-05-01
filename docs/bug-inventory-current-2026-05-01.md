# Current Bug / Tech Debt Inventory — 2026-05-01

This is the current working inventory after the 2026-04-30 verified bug pass,
the P2 hardening wave, and the 2026-05-01 architecture hygiene packages.

Use this file for new planning. Older inventories remain useful as historical
review evidence, but their line numbers and "remaining" counts are stale.

## Current Baseline

Latest verified main commits:

| Commit | Scope | CI |
| --- | --- | --- |
| `c415bad` | Documentation truth refresh | `25198760941` success |
| `f6396f2` | Remove Python placeholder module ambiguity | `25198892159` success |
| `a58907b` | Split Rust service helper boundaries | `25199109107` success |
| `2fea866` | Split Python NETCONF backend helpers | `25199378631` success |

Current local verification at the time this inventory was created:

- Python adapter tests: `232 passed`
- Focused package / NETCONF tests: `59 passed`
- `git diff --check`: passed
- Local Rust toolchain is unavailable in this workspace; Rust compile/test
  status is verified by GitHub Actions.

## Resolved Since Older Inventories

These older claims should not be re-opened unless new evidence appears:

- Rust transaction shadow/journal ordering and terminal status handling.
- Partial apply failure incorrectly aggregating as `SuccessWithWarning`.
- Batch recovery journal-read / lock TOCTOU.
- Recovery attempt context being overwritten when entering `InDoubt`.
- Secret orphan cleanup on registration/bootstrap failure.
- Adapter client connection churn; `AdapterClientPool` is now used.
- Drifted lifecycle not clearing after a clean audit.
- Desired baseline and observed cache being mixed in one shadow meaning.
- File-backed journal/shadow durability and concurrent same-record writes.
- `HostKeyPolicy` transport from Rust to Python.
- TOFU host-key policy behaving as strict known-hosts only.
- Python placeholder modules for unimplemented NAPALM / Netmiko / diff /
  rollback / state paths.
- Rust dead renderer skeletons.
- Python NETCONF backend helper sprawl in a single 1000+ line file.

## Open P1 Items

| Area | Item | Why it matters | Next action |
| --- | --- | --- | --- |
| Transactions | Crash/restart matrix still needs process-level chaos coverage | File-backed restart coverage now includes pending recovery, `ForceResolved` restart, successful shadow persistence, terminal `Committed` / `Failed` / `RolledBack` filtering, corrupt journal/shadow fail-closed behavior, and `.tmp` residue handling. The remaining gap is process-kill / adapter session-drop chaos rather than basic store recreation. | Add a process-level crash harness and adapter session-drop scenarios when the test infrastructure can launch/kill the service process. |
| Rust API architecture | `AriaUnderlayService` still owns too much orchestration | Pure helpers were split out, but apply/recovery/admin flows are still concentrated in one facade. This makes transaction review harder as features grow. | Extract apply, recovery, and admin coordinators without changing public service behavior. |
| Operations | Audit/metrics are not yet operator-facing enough | Force-resolve, drift, GC, and recovery emit events, but operators still lack a compact queryable summary of what happened and what requires action. | Add consistent operation summaries and telemetry counters for force-resolve, recovery, drift audit, and GC. |
| GC | GC is implemented but not fully productionized | `run_once` and retention policy exist; production still needs scheduling, quota thinking, and clearer audit output. | Wire GC into a periodic worker path and expose retention / deletion summaries. |
| Drift | Drift auditor lacks a full background operational loop | Current one-shot audit works, but production needs cadence, alerting, and clear lifecycle/reporting semantics. | Add scheduler-facing summary and metrics; keep AutoReconcile fail-closed until explicitly designed. |
| Real NETCONF parser | Huawei/H3C state parsers are fixture-verified only | Fixture XML proves parser boundaries, not real device namespace and field behavior. | When hardware is available, collect running XML and promote only after validator + tests pass. |
| Real NETCONF renderer | Huawei/H3C renderers are still skeletons | Snapshot rendering is useful, but real devices may reject the XML. | Keep production prepare fail-closed; add vendor profile tests and later real-device validation. |

## Open P2 Items

| Area | Item | Why it matters | Next action |
| --- | --- | --- | --- |
| Host key policy | Fingerprint-only pinned host key is still unsupported | Rust can carry the policy, but Python ncclient exact pinning support does not match the stored fingerprint shape. | Design exact fingerprint verification or change model semantics; keep fail-closed until then. |
| Drift policy | `AutoReconcile` remains explicitly unimplemented | This is correct for safety, but the enum exists and operators may expect behavior later. | Keep returning a clear unsupported error; design separately with approval gates. |
| Vendor scope | Cisco/Ruijie renderer and parser are not implemented | Framework is ready, but no vendor samples/profiles exist. | Wait for samples or explicit profile requirements. |
| Alternate backends | NAPALM / Netmiko / SSH CLI are not implemented | They are roadmap items, not current paths. | Add real modules only when there is a supported backend contract and tests. |
| Force unlock | NETCONF force unlock / kill-session is not implemented | Current unsupported result is safer than pretending success, but it is an operations gap. | Design with device/session identity and audit requirements before implementation. |
| Docs | `docs/device-capability-report.md` still contains TODO placeholders | This is not runtime risk, but it weakens operator documentation. | Fill or remove TODO sections once capability reporting semantics settle. |
| Test hygiene | Some older low-risk test helpers rely on fragile assumptions | They do not block functionality but make future refactors noisier. | Clean opportunistically during related test work. |

## Current Execution Plan

The next no-real-switch sequence is:

1. Expand transaction crash/restart tests.
2. Continue splitting Rust service orchestration into coordinators.
3. Add audit/metrics operation summaries.
4. Revisit real-device parser/renderer only after hardware or captured XML is
   available.
