# Current Bug / Tech Debt Inventory — 2026-05-01

This is the current working inventory after the 2026-04-30 verified bug pass,
the P2 hardening wave, and the 2026-05-01 architecture hygiene packages.

Use this file for new planning. Older inventories remain useful as historical
review evidence, but their line numbers and "remaining" counts are stale.

## Current Baseline

Latest verified main commits:

| Commit | Scope | CI |
| --- | --- | --- |
| `9461c95` | Transaction process chaos coverage package 1 | `25211232450` success |
| `351b449` | Journal GC worker productionization package | `25211412687` success |
| `d56a3c9` | Drift audit worker loop package | `25211569832` success |
| `7934e8d` | Operation summary query surface package | `25211804565` success |
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
| Transactions | Crash/restart matrix still needs broader process-level chaos coverage | File-backed restart coverage now includes pending recovery, `ForceResolved` restart, successful shadow persistence, terminal `Committed` / `Failed` / `RolledBack` filtering, corrupt journal/shadow fail-closed behavior, `.tmp` residue handling, process children that exit during `Preparing`, `Committing`, `Verifying`, `FinalConfirming`, and `RollingBack`, a multi-device `Committing` recovery record with mixed adapter outcomes, and transient recover transport retry before `InDoubt`. Recovery now writes shadow before terminal `Committed` journal on roll-forward. Remaining gaps are longer multi-attempt reconnect/backoff sequences and real adapter behavior under session churn. | Extend reconnect/backoff coverage if needed; otherwise shift to persistent audit sink or daemon lifecycle integration. |
| Rust API architecture | `AriaUnderlayService` still needs a thinner facade over time | Apply, recovery, and admin-operation coordinators now own the main orchestration flows. The remaining architecture work is to keep future flows from leaking back into the facade and to split drift audit if it grows. | Keep new transaction/admin logic in the coordinator modules; consider a dedicated drift coordinator if the audit loop expands. |
| Operations | Audit/metrics still need production audit integration | Force-resolve, drift audit, GC, recovery, and transaction InDoubt events now map into service-queryable operation summaries and metrics counters. A JSONL file-backed `OperationSummaryStore` exists for restart-safe local persistence, with record-count/byte retention, archive rotation, fail-closed corrupt-record handling, daemon-scheduled compaction, `audit.write_failed` observability, service-level result filtering, aggregate overview counts, formal local `aria-underlay-ops` read commands, checked-in daemon config sample, operator runbook, internal JSONL operation alert storage with checkpointed dedupe, RBAC/product-audit enforcement for `force_resolve_transaction`, internal alert lifecycle actions, local and product-API worker config retention/schedule changes, `ProductOpsManager` RBAC gates for product-facing summary reads, product audit export, and worker config changes, `ProductOpsApi` handler-facing request/session facade, framework-neutral `ProductHttpRouter` method/path/status/body JSON contract, `BearerTokenProductSessionExtractor` plus `ProductIdentityVerifier` identity boundary, offline JWT/JWKS signature and claims verification, and a local `ProductHttpServer` / `aria-underlay-product-api` HTTP/1.1 listener package. Remaining work is the real product audit database, online IdP/OIDC discovery or JWKS refresh, production TLS/ingress/server packaging, product UI/API packaging, and online daemon hot reload. External webhook, enterprise IM, PagerDuty, and email delivery are intentionally out of scope. | Keep privileged operation changes behind `AuthorizationPolicy` and `ProductAuditStore`; keep the checked-in listener local until production ingress and JWKS/key-refresh workflow are implemented. |
| GC | GC still needs external deployment integration | `run_once`, retention policy, periodic worker entrypoint, event emission, deletion summaries, shared `UnderlayWorkerRuntime`, the `aria-underlay-worker` JSON-configured daemon binary, production-style JSON sample, systemd sample, tmpfiles.d sample, and offline `check-worker-config` preflight now exist. Production still needs real package installation, host user creation, service enablement, persistent audit policy, and site-specific disk quota policy. | Keep deployment samples tested; add real packaging only when target OS/package format is selected. |
| Drift | Drift auditor still needs external deployment integration | One-shot audit, scheduler-facing summary, event emission, periodic worker entrypoint, shared `UnderlayWorkerRuntime`, the `aria-underlay-worker` JSON-configured daemon binary, internal alert lifecycle triage, production-style JSON sample, systemd sample, tmpfiles.d sample, and offline `check-worker-config` preflight now exist. Production still needs real package installation and system service ownership in the target environment. | Keep AutoReconcile fail-closed until explicitly designed; add real packaging only after target deployment platform is selected. |
| Real NETCONF parser | Huawei/H3C state parsers are fixture-verified only | Fixture XML proves parser boundaries, not real device namespace and field behavior. | When hardware is available, collect running XML and promote only after validator + tests pass. |
| Real NETCONF renderer | Huawei/H3C renderers are still skeletons | Snapshot rendering is useful, but real devices may reject the XML. Renderer skeletons now validate profile fields, reject production-ready skeleton markers, keep VLAN/interface namespaces distinct, and have snapshot negative coverage for invalid trunk mode. | Keep production prepare fail-closed; continue adding vendor profile/snapshot tests as samples arrive, then promote only after real-device validation. |

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

1. Add online IdP/OIDC discovery or JWKS refresh if key rotation must happen
   outside config deployment.
2. Harden production ingress/server packaging around `ProductHttpServer` or replace
   it with the selected web framework adapter while keeping the route contract.
3. Design online daemon hot reload separately from file-backed config mutation.
4. Select target deployment packaging format before turning the systemd/tmpfiles
   samples into an installer.
5. Revisit real-device parser/renderer only after hardware or captured XML is
   available.
