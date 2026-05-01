# Bug Inventory — 2026-04-30

Verified bug findings from two-pass code review, refreshed after the
2026-04-30 adapter pool, secret cleanup, dry-run, parser fixture work, and
the 2026-04-30 deep whole-code review.
Focus: transaction recovery correctness, device/bootstrap, intent/validation,
error handling, Python adapter boundary.

**Verification methodology:** Each finding traced through the full call chain
(Rust → Python gRPC or Rust → Rust). Older claims from
`bug-inventory-2026-04-26.md` were rechecked against current code instead of
trusted as-is.

## Current Status Matrix

| Priority | Finding | Status | Current owner |
| --- | --- | --- | --- |
| P0 | ConfirmedCommit recovery blind spot after `FinalConfirming` | Fixed in `8d31421`; GitHub Actions run `25173697368` passed | Rust transaction recovery |
| P1 | `HostKeyPolicy` not wired from Rust `DeviceInfo` to Python NETCONF backend | Fixed in `bb1158c`; GitHub Actions run `25174727686` passed | Proto + Rust mapper + Python server/backend |
| P1 | Drifted lifecycle is never cleared after a clean drift audit | Fixed in `b4e1a6e`; GitHub Actions run `25176896290` passed | Rust drift audit + inventory lifecycle |
| P1 | `ShadowStateStore` mixes desired baseline and observed cache, masking drift | Fixed in `b4e1a6e`; GitHub Actions run `25176896290` passed | Rust shadow/drift/preflight state model |
| P1 | File-backed shadow path sanitization can collide distinct device IDs | Fixed in `8ca1aec`; GitHub Actions run `25175796255` passed | Rust shadow persistence + device ID validation |
| P1 | File-backed journal/shadow writes are not production-durable under concurrent writes or crash | Fixed in `3f15941`; GitHub Actions run `25176390555` passed | Rust journal/shadow persistence |
| P1 | Product initialization/register accepts invalid connection inputs | Fixed in `8ca1aec`; GitHub Actions run `25175796255` passed | Rust bootstrap/registration validation |
| P1 | Additional adapter error details dropped from journal diagnostics | Fixed in `e76e961`; GitHub Actions run `25176070652` passed | Rust error/journal mapping |
| P2 | `TrustOnFirstUse` currently behaves as strict known-hosts verification, not TOFU | Fixed in `817607d`; GitHub Actions run `25197588455` passed | Python NETCONF backend + host-key trust store |
| P2 | Rust `device/render.rs` renderer skeletons are dead code | Fixed in `d286f3b`; GitHub Actions run `25198279834` passed | Rust device module |
| P2 | Python vendor driver stubs raise `NotImplementedError` on construction | Fixed in `a3cc43a`; GitHub Actions run `25197914750` passed | Python driver registry |
| P2 | `_admin_state_to_text` duplicated with inconsistent defaults | Fixed in `a3cc43a`; GitHub Actions run `25197914750` passed | Python backend/renderer shared helper |
| P2 | `SmallFabric` topology lacks explicit endpoint count semantics | Fixed in `9808ef7`; GitHub Actions run `25198141224` passed | Intent validation + requirements docs |
| P2 | endpoint lock jitter uses time-derived modulo instead of independent PRNG | Fixed in `9808ef7`; GitHub Actions run `25198141224` passed | Rust endpoint lock |
| P2 | scope VLAN `int()` conversion has defensive error-message gap | Fixed in `a3cc43a`; GitHub Actions run `25197914750` passed | Python state scope helpers |

## Newly Confirmed 2026-04-30 Deep Review Findings

These are current-code findings, not carried over from older notes. The P1
items below were fixed after this review; remaining open items are P2 unless
their section says otherwise.

## RESOLVED MEDIUM — Drifted Lifecycle Is Never Cleared After Clean Audit

**Files:** `src/api/service.rs:765-807`, `src/api/service.rs:1190-1217`, `docs/implementation-plan.md:480-489`

**Root cause:** `run_drift_audit()` marks a device `Drifted` when the drift
report contains findings, but the no-drift branch only writes the observed
state to `shadow_store`. It never transitions the inventory lifecycle back from
`DeviceLifecycleState::Drifted` to `Ready`. The implementation plan explicitly
lists `Drifted -> Ready` as a required lifecycle transition.

**Impact:** A device that drifted once can remain blocked forever when
`ApplyOptions.drift_policy = BlockNewTransaction`, even after a later audit
proves the observed state matches the baseline. Operators would need an
unrelated manual lifecycle mutation to unblock normal writes.

**Fix direction:** In the clean-audit branch, update lifecycle from `Drifted`
to `Ready` after persisting the clean observation. Add a regression test that
starts from `Drifted`, runs a no-drift audit, and verifies
`BlockNewTransaction` no longer blocks that device. Be careful not to force
`Unsupported`, `Unreachable`, `AuthFailed`, or `Maintenance` devices to `Ready`.

**Resolution 2026-04-30:** Fixed in `b4e1a6e`; GitHub Actions run
`25176896290` passed. `run_drift_audit()` now clears a `Drifted` lifecycle back
to `Ready` only after a clean comparison against an existing desired baseline.
Regression coverage lives in `tests/transaction_gate_tests.rs`.

---

## RESOLVED MEDIUM — ShadowStateStore Masks Drift By Mixing Observed Cache With Desired Baseline

**Files:** `src/api/service.rs:203-217`, `src/api/service.rs:389-390`, `src/api/service.rs:1091-1094`, `src/api/service.rs:1190-1217`, `docs/aria-underlay-requirements.md:866-900`

**Root cause:** The same `ShadowStateStore` is used for two different meanings:

- expected desired baseline after successful apply (`DeviceShadowState::from_desired`)
- latest observed running state after refresh, preflight, or clean drift audit

`fetch_current_states()` and `get_device_state()` write the adapter-observed
running state into shadow. `run_drift_audit()` then reads shadow as the
expected baseline. If an out-of-band change is refreshed or preflighted before
the drift audit, shadow is overwritten with that out-of-band state and the
audit can no longer detect the drift.

**Impact:** Manual device changes can be normalized into the expected baseline
by a refresh or dry-run/preflight. This violates the requirement that drift
audit compares observed running state against the expected Aria-owned baseline.

**Fix direction:** Split the model into at least two persisted records:
`DesiredBaselineStore` (only updated by successful apply or explicit operator
acceptance) and `ObservedStateCache` (updated by refresh/preflight/audit). Drift
audit must compare observed cache/current read against desired baseline. If the
project keeps one physical store, add an explicit `source`/`kind` dimension and
make drift audit ignore observed-cache entries as expected baselines.

**Resolution 2026-04-30:** Fixed in `b4e1a6e`; GitHub Actions run
`25176896290` passed. `shadow_store` is now treated as the desired baseline,
while `observed_store` caches refresh/preflight/audit observations. Refresh and
dry-run no longer overwrite the desired baseline, and drift audit compares a
fresh observed state against the baseline.

---

## RESOLVED MEDIUM — File Shadow Store Path Sanitization Collides Device IDs

**Files:** `src/state/shadow.rs:95-124`, `src/state/shadow.rs:163-172`, `src/intent/validation.rs:147-152`, `src/device/bootstrap.rs:228-253`

**Root cause:** `shadow_file_stem()` replaces every character outside
`[A-Za-z0-9_-]` with `_`. Device IDs are only checked for non-empty strings and
exact duplicate equality in the current validation paths. Distinct IDs such as
`leaf/a` and `leaf_a` both map to `leaf_a.json`.

**Impact:** Two valid-but-different device IDs can overwrite or read each
other's file-backed shadow state. That can corrupt preflight diffs, drift
audits, and transaction recovery decisions for persisted deployments.

**Fix direction:** Prefer fail-closed ID validation: define one canonical
device ID character set and reject IDs that cannot be stored losslessly. If
compatibility requires arbitrary IDs, encode file stems collision-free (for
example URL-safe base64 or a hex digest plus escaped display prefix) and add
collision regression tests.

**Resolution 2026-04-30:** Fixed in `8ca1aec`; GitHub Actions run
`25175796255` passed. Device IDs now have a canonical identifier rule enforced
by registration, bootstrap/domain intent validation, and the file-backed shadow
store. Non-canonical IDs fail closed instead of being lossy-sanitized into
colliding filenames.

---

## RESOLVED MEDIUM — File-Backed Journal/Shadow Writes Are Single-Writer And Crash-Weak

**Files:** `src/state/shadow.rs:118-124`, `src/tx/journal.rs:225-234`

**Root cause:** Both file-backed stores write through a fixed temp filename:
`path.with_extension("json.tmp")`, then rename it into place. Concurrent writes
for the same record share the same temp path. The write path also uses
`fs::write()` + `fs::rename()` without syncing the temp file or parent
directory.

**Impact:** Concurrent writers can race on the same temp file, causing one
writer to rename another writer's payload or fail with a missing temp file.
After a process or power crash, a returned `rename()` is not enough to prove the
journal/shadow update reached durable storage on all filesystems. This weakens
the transaction durability story for file-backed mode.

**Fix direction:** Use per-write unique temp files in the same directory and
serialize same-record writes with an in-process lock. For production durability,
write through `File`, `sync_all()` the temp file, `rename()`, then fsync the
parent directory. Add tests for concurrent same-device shadow writes and
document whether cross-process safety is supported.

**Resolution 2026-04-30:** Fixed in `3f15941`; GitHub Actions run
`25176390555` passed. File-backed journal and shadow writes now use unique temp
files, temp-file fsync, atomic rename, parent-directory fsync, and in-process
per-record locks. Regression tests cover concurrent same-device shadow writes
and concurrent same-transaction journal writes.

---

## RESOLVED MEDIUM — Product Initialization/Register Accepts Invalid Connection Inputs

**Files:** `src/device/bootstrap.rs:104-164`, `src/device/bootstrap.rs:228-253`, `src/device/registration.rs:38-55`, `adapter-python/aria_underlay_adapter/server.py:186-194`

**Root cause:** `validate_switch_pair()` only validates switch count, LeafA /
LeafB shape, and duplicate device IDs. The bootstrap and registration path does
not reject empty `request_id`, `tenant_id`, `site_id`, empty
`adapter_endpoint`, empty/unsafe `device_id`, empty `management_ip`, or
`management_port == 0`. `DeviceRegistrationService::register()` persists the
request directly. Python then defaults `device.management_port or 830`, which
can hide a bad port `0` instead of surfacing invalid inventory.

**Impact:** Invalid device records can be created and then fail later during
onboarding/apply with less actionable adapter errors. Some invalid IDs also
interact with the file-stem collision issue above.

**Fix direction:** Add shared validation for registration and bootstrap:
non-empty tenant/site/request/adapter endpoint, canonical device ID charset,
non-empty management host, non-zero port, and valid host-key policy payloads.
Registration should fail before secret creation where possible; when validation
after secret creation is unavoidable, keep the existing compensating secret
cleanup path.

**Resolution 2026-04-30:** Fixed in `8ca1aec`; GitHub Actions run
`25175796255` passed. Registration and bootstrap now reject empty tenant/site,
empty or non-canonical device IDs, empty management host, zero management port,
invalid adapter endpoints, and invalid host-key-policy payloads before
persistence.

---

## RESOLVED MEDIUM — UnderlayError.errors Diagnostic Data Silently Dropped

**Files:** `src/error.rs:21-27`, `src/adapter_client/mapper.rs:12-30`, `src/api/service.rs:1520-1522`

**Root cause:** `UnderlayError::AdapterOperation` has an
`errors: Vec<AdapterErrorDetail>` field populated by `extract_adapter_errors()`
— it captures all but the first adapter error detail. However
`journal_error_fields()` uses `..` to discard all fields it does not explicitly
enumerate. The `errors` vec is populated correctly but never read. The
`#[allow(dead_code)]` annotation confirms the field is currently unused.

**Impact:** When a multi-error adapter failure occurs, only the first error is
visible in result/journal paths. Root-cause diagnosis loses important context,
especially when Python returns both normalized and raw sub-errors.

**Fix direction:** Include additional adapter error details in journal messages
and apply result messages in a bounded form. Keep a structured form if possible
when later extending the journal schema.

**Resolution 2026-04-30:** Fixed in `e76e961`; GitHub Actions run
`25176070652` passed. `journal_error_fields()` now preserves bounded additional
adapter error details in the journal/apply message while keeping the primary
adapter error code stable.

---

## RESOLVED LOW — TrustOnFirstUse Is Fail-Closed But Not Actual TOFU

**Files:** `src/device/info.rs:6-9`, `src/adapter_client/mapper.rs:50-69`, `adapter-python/aria_underlay_adapter/server.py:199-205`, `adapter-python/aria_underlay_adapter/backends/netconf.py:67-108`

**Root cause:** `HostKeyPolicy::TrustOnFirstUse` is now transported from Rust to
Python, but Python maps it to `hostkey_verify=True` without an
`unknown_host_cb` or persistent trust store. In practice this behaves like
strict known-hosts verification for unknown hosts, not "trust and persist on
first use."

**Impact:** This is safe from a security perspective because it fails closed,
but the API semantics are misleading. A device configured for TOFU may fail its
first connection unless the host key is already present in the runtime known
hosts set.

**Fix direction:** Either rename/document the current behavior as
`StrictKnownHostsDefault`, or implement real TOFU with a durable trust store:
on unknown host, persist the first key atomically, verify subsequent keys
against the stored key, and audit first-use acceptance.

**Resolution 2026-05-01:** Fixed in `817607d`; GitHub Actions run
`25197588455` passed. Python adapter now has
a configurable TOFU trust store (`ARIA_UNDERLAY_TOFU_KNOWN_HOSTS_FILE`, default
`/tmp/aria-underlay-adapter/tofu_known_hosts`). First use persists the observed
remote host key atomically before returning the session; later connects use
strict known-hosts verification. Missing remote key, trust-store write failure,
or conflicting existing key fails closed.

---

## Superseded 2026-04-26 Claims

The following older findings were rechecked and are no longer open:

- `aggregate_apply_status` partial failure as `SuccessWithWarning`: fixed; mixed
  success/failure now aggregates to `Failed`.
- journal `Committed` before shadow write: fixed; shadow is written before the
  terminal `Committed` journal record.
- recovery list/read TOCTOU without re-read after lock: fixed; recovery re-reads
  the candidate record after endpoint lock acquisition.
- recovery attempt history lost: fixed with `TxJournalRecord.error_history`.
- secret orphan after registration failure: fixed with compensating secret
  cleanup.
- adapter client connection churn: fixed with `AdapterClientPool`.
- mock `_normalize_mode` silently converting unknown modes: fixed; unknown modes
  now fail closed.
- `_discard_candidate` swallowing the original error: fixed; discard failure is
  appended to the original error context.
- broad auth substring matching: fixed with class/phrase-based classification.
- `_port_mode_to_proto(None)` losing unset-vs-zero: not reproduced; current
  protobuf optional scalar handling preserves unset when passed `None`.

---

## RESOLVED HIGH — ConfirmedCommit Recovery Blind Spot

**Files:** `src/api/service.rs:644-648, 423-426`, `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:270-331`, `adapter-python/aria_underlay_adapter/backends/netconf.py:358-380`

**Root cause:** When `final_confirm` succeeds on the adapter (NETCONF persist-id consumed, commit confirmed on the switch) but the journal `Committed` write at `service.rs:426` fails (or process crashes in between), the journal stays at `FinalConfirming` (written at line 644, BEFORE the `final_confirm` call).

Recovery classifies `FinalConfirming` as `AdapterRecover`, which calls `NetconfBackedDriver.recover()`. That method always delegates to `backend.rollback_candidate(strategy=ConfirmedCommit, tx_id=tx_id)`, which calls `session.cancel_commit(persist_id=tx_id)`. This inevitably fails because the persist-id was already consumed by the successful `final_confirm`.

**Result:** Transaction permanently stuck in `InDoubt`. Every subsequent recovery hits the same wall. Only `force_resolve_transaction` (break-glass) can clear it.

**Crash window:** Between `final_confirm_with_context` returning `Ok` at `service.rs:646-648` and `self.journal.put(Committed)` at line 426.

**Why normal rollback works:** If crash happens BEFORE `final_confirm` (journal is `Verifying` or `Committing`), the confirmed commit is still pending on the switch. `cancel_commit` succeeds, rollback works, transaction resolves to `RolledBack`. The blind spot is specifically post-final_confirm, pre-journal-write.

**Fix status:** Fixed in `8d31421`; GitHub Actions run `25173697368` passed.

The implemented fix is deliberately narrower and safer than treating every
"unknown persist-id" as committed:

1. `TxJournalRecord` now persists recovery-safe `desired_states` and
   `change_sets`.
2. `apply_single_endpoint_state()` stores the desired state and touched-resource
   change set before the first journal write.
3. `FinalConfirming + ConfirmedCommit` recovery now uses a dedicated path:
   retry `final_confirm`, then verify the persisted desired state using the
   persisted change-set scope, then fall back to existing adapter recover.
4. If none of those paths can prove `Committed` or `RolledBack`, recovery writes
   an explicit `FINAL_CONFIRM_RECOVERY_IN_DOUBT` error instead of silently
   looping through blind `cancel-commit`.

This avoids incorrectly marking a transaction committed when the confirmed
commit expired and the device rolled back before recovery.

---

## RESOLVED MEDIUM — HostKeyPolicy Not Wired From Rust to Python NETCONF Backend

**Files:** `src/device/info.rs` (HostKeyPolicy enum definition), `src/adapter_client/mapper.rs:34-43` (`device_ref_from_info`), `adapter-python/aria_underlay_adapter/server.py:173-191` (`_netconf_driver_from_device`)

**Root cause:** The Rust model defines `HostKeyPolicy::TOFU` / `KnownHostsFile` / `PinnedKey` in `DeviceInfo`, and `DeviceRegistrationService` accepts it via `RegisterDeviceRequest`. However `device_ref_from_info()` — the mapper that converts Rust `DeviceInfo` to the protobuf `DeviceRef` sent to the Python adapter — does NOT include `host_key_policy`. On the Python side, `_netconf_driver_from_device` hardcodes `hostkey_verify=False`.

**Impact:** All NETCONF connections use `hostkey_verify=False` regardless of configured policy. No TOFU enforcement, no known-hosts checking.

**Fix direction:** Add `host_key_policy` to the `DeviceRef` protobuf message, populate it in `device_ref_from_info()`, consume in `_netconf_driver_from_device`.

**Resolution 2026-04-30:** Fixed in `bb1158c`; GitHub Actions run
`25174727686` passed. `DeviceRef` now carries `host_key_policy`,
`known_hosts_path`, and `pinned_host_key_fingerprint`. Rust
`device_ref_from_info()` maps all three `HostKeyPolicy` variants. Python
`_netconf_driver_from_device()` passes the policy into `NcclientNetconfBackend`.
`KnownHostsFile` is enforced with `hostkey_verify=True` and an ncclient SSH
config shim for `UserKnownHostsFile`. `PinnedKey` is transported but session
opening fails closed with `HOST_KEY_PINNING_UNSUPPORTED` because ncclient exposes
exact `hostkey_b64` pinning while the Rust model currently stores only a
fingerprint.

---

## RESOLVED LOW — Rust DeviceConfigRenderer Dead Code

**Files:** `src/device/render.rs` (all 99 lines)

**Finding:** `CiscoRenderer`, `H3cRenderer`, `HuaweiRenderer`, `RuijieRenderer` all implement `DeviceConfigRenderer` by returning `Err(UnderlayError::UnsupportedOperation)`. `grep -rn` across `src/` confirms zero callers outside `render.rs` and `mod.rs` (the re-export). `renderer_for_vendor()` constructs dead objects. Actual rendering is Python-side via gRPC.

**Fix direction:** Remove the four renderer structs, the trait, and `renderer_for_vendor()`, or wire them to the Python adapter.

**Resolution 2026-05-01:** Fixed in `d286f3b`; GitHub Actions run
`25198279834` passed. The Rust
`device/render.rs` module, its re-exports, and the tests that only exercised
the dead skeletons were removed. Production rendering remains Python-side via
the adapter renderer registry.

---

## RESOLVED LOW — _admin_state_to_text: Three Implementations, Two Behaviors

**Files:** `backends/netconf.py:810-815`, `backends/mock_netconf.py:510-515`, `renderers/common.py:189-194`

**Finding:** Three copies of `_admin_state_to_text` exist with different behaviors:
- `netconf.py`: `int(value or 0) == 2` → "down", else "up" (UNSPECIFIED=0 → "up")
- `mock_netconf.py`: exact match only, else `str(value).lower()` (UNSPECIFIED=0 → "0")
- `renderers/common.py`: same pattern as mock_netconf.py

Currently unreachable in production because `interface_to_proto` only maps `Up(1)`/`Down(2)`. Test/mock fidelity gap only.

**Fix direction:** Consolidate to a single shared implementation.

**Resolution 2026-05-01:** Fixed in `a3cc43a`; GitHub Actions run
`25197914750` passed. NETCONF,
mock backend, and renderer code now share one `admin_state_to_text` helper.
Unspecified/zero admin state is normalized as `"up"` consistently, and
regression coverage checks NETCONF, mock, and renderer callers.

---

## RESOLVED LOW — Five Python Driver Stubs That Crash on Construction

**Files:** `drivers/ruijie.py:5`, `drivers/legacy_cli.py:5`, `drivers/h3c.py:5`, `drivers/cisco.py:5`, `drivers/huawei.py:5`

**Finding:** Each class `__init__` immediately raises `NotImplementedError`. Any code path that instantiates them will panic at runtime rather than getting a clear "unsupported vendor" error.

**Fix direction:** Move the error to the driver factory so unsupported vendors fail at lookup time.

**Resolution 2026-05-01:** Fixed in `a3cc43a`; GitHub Actions run
`25197914750` passed. Unsupported
vendor stubs are now constructable and fail closed when an operation is invoked,
so registry/import paths can inspect them without triggering construction-time
panics.

---

## RESOLVED LOW — SmallFabric Topology Skips Endpoint Count Validation

**Files:** `src/intent/validation.rs:129-144`

**Finding:** `validate_topology_shape` checks `StackSingleManagementIp` (requires exactly 1 endpoint) and `MlagDualManagementIp` (requires exactly 2 endpoints), but `SmallFabric` falls through to `_ => Ok(())` with no validation. Any number of endpoints passes.

**Fix direction:** Add minimum endpoint count validation for SmallFabric, or document the intended range.

**Resolution 2026-05-01:** Fixed in `9808ef7`; GitHub Actions run
`25198141224` passed.
`SmallFabric` now has explicit endpoint semantics: at least two management
endpoints and no hard-coded upper bound. Single-endpoint deployments should use
`StackSingleManagementIp`.

---

## RESOLVED LOW — Jitter Source Not Independently Random

**Files:** `src/tx/endpoint_lock.rs:68-75`

**Finding:** Jitter uses `SystemTime::now().subsec_nanos()` directly. Concurrent callers within the same sub-second window get correlated jitter values, slightly reducing thundering-herd protection. Exponential backoff still provides primary protection. Low practical impact.

**Fix direction:** Use a proper PRNG (e.g., `rand::thread_rng()`) for independent jitter values.

**Resolution 2026-05-01:** Fixed in `9808ef7`; GitHub Actions run
`25198141224` passed. Endpoint
lock backoff now uses `rand::thread_rng()` through an `add_jitter` helper
instead of deriving jitter from wall-clock nanoseconds. Unit coverage verifies
the helper keeps jitter within the documented 25% bound.

---

## RESOLVED LOW — _normalized_scope_vlan_ids Defensive Gap

**Files:** `backends/netconf.py:649`, `renderers/common.py:105`

**Finding:** `int(vlan_id)` in set comprehensions without try/except. Protobuf uint32 prevents non-numeric values, so no current trigger. Defensive hardening only.

**Fix direction:** Add explicit error message if `int()` conversion fails.

**Resolution 2026-05-01:** Fixed in `a3cc43a`; GitHub Actions run
`25197914750` passed. NETCONF
state filter construction and fixture state parser scope filtering now convert
scope VLAN IDs with per-index error context and return structured AdapterError
instead of leaking a bare `ValueError`.
