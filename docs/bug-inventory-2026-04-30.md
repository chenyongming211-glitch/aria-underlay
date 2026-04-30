# Bug Inventory — 2026-04-30

Verified bug findings from two-pass code review, refreshed after the
2026-04-30 adapter pool, secret cleanup, dry-run, and parser fixture work.
Focus: transaction recovery correctness, device/bootstrap, intent/validation,
error handling, Python adapter boundary.

**Verification methodology:** Each finding traced through the full call chain
(Rust → Python gRPC or Rust → Rust). Older claims from
`bug-inventory-2026-04-26.md` were rechecked against current code instead of
trusted as-is.

## Current Status Matrix

| Priority | Finding | Status | Current owner |
| --- | --- | --- | --- |
| P0 | ConfirmedCommit recovery blind spot after `FinalConfirming` | Fixed in current hardening change set; CI validation required because local Rust toolchain is unavailable | Rust transaction recovery |
| P1 | `HostKeyPolicy` not wired from Rust `DeviceInfo` to Python NETCONF backend | Confirmed | Proto + Rust mapper + Python server/backend |
| P1 | Additional adapter error details dropped from journal diagnostics | Confirmed | Rust error/journal mapping |
| P2 | Rust `device/render.rs` renderer skeletons are dead code | Confirmed low-risk tech debt | Rust device module |
| P2 | Python vendor driver stubs raise `NotImplementedError` on construction | Confirmed low-risk tech debt | Python driver registry |
| P2 | `_admin_state_to_text` duplicated with inconsistent defaults | Confirmed low-risk fidelity gap | Python backend/renderer shared helper |
| P2 | `SmallFabric` topology lacks explicit endpoint count semantics | Confirmed ambiguity | Intent validation + requirements docs |
| P2 | endpoint lock jitter uses time-derived modulo instead of independent PRNG | Confirmed low-risk contention hardening | Rust endpoint lock |
| P2 | scope VLAN `int()` conversion has defensive error-message gap | Confirmed low-risk hardening | Python state scope helpers |

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

## HIGH — ConfirmedCommit Recovery Blind Spot

**Files:** `src/api/service.rs:644-648, 423-426`, `adapter-python/aria_underlay_adapter/drivers/netconf_backed.py:270-331`, `adapter-python/aria_underlay_adapter/backends/netconf.py:358-380`

**Root cause:** When `final_confirm` succeeds on the adapter (NETCONF persist-id consumed, commit confirmed on the switch) but the journal `Committed` write at `service.rs:426` fails (or process crashes in between), the journal stays at `FinalConfirming` (written at line 644, BEFORE the `final_confirm` call).

Recovery classifies `FinalConfirming` as `AdapterRecover`, which calls `NetconfBackedDriver.recover()`. That method always delegates to `backend.rollback_candidate(strategy=ConfirmedCommit, tx_id=tx_id)`, which calls `session.cancel_commit(persist_id=tx_id)`. This inevitably fails because the persist-id was already consumed by the successful `final_confirm`.

**Result:** Transaction permanently stuck in `InDoubt`. Every subsequent recovery hits the same wall. Only `force_resolve_transaction` (break-glass) can clear it.

**Crash window:** Between `final_confirm_with_context` returning `Ok` at `service.rs:646-648` and `self.journal.put(Committed)` at line 426.

**Why normal rollback works:** If crash happens BEFORE `final_confirm` (journal is `Verifying` or `Committing`), the confirmed commit is still pending on the switch. `cancel_commit` succeeds, rollback works, transaction resolves to `RolledBack`. The blind spot is specifically post-final_confirm, pre-journal-write.

**Fix status:** Fixed in the current hardening change set.

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

## MEDIUM — UnderlayError.errors Diagnostic Data Silently Dropped

**Files:** `src/error.rs:19-25`, `src/adapter_client/mapper.rs:13-32`, `src/api/service.rs:1336-1356`

**Root cause:** `UnderlayError::AdapterOperation` has an `errors: Vec<AdapterErrorDetail>` field populated by `extract_adapter_errors()` — it captures all but the first adapter error detail. However `journal_error_fields()` at `service.rs:1355` uses `..` to discard all fields it doesn't explicitly enumerate. The `errors` vec is populated correctly but never read. The `#[allow(dead_code)]` annotation on line 23 confirms this was known at write time.

**Impact:** When a multi-error adapter failure occurs, only the first error is visible in logs/journal. Root-cause diagnosis loses context.

**Fix direction:** Include `errors` in `journal_error_fields` output, or log them at minimum.

---

## MEDIUM — HostKeyPolicy Not Wired From Rust to Python NETCONF Backend

**Files:** `src/device/info.rs` (HostKeyPolicy enum definition), `src/adapter_client/mapper.rs:34-43` (`device_ref_from_info`), `adapter-python/aria_underlay_adapter/server.py:173-191` (`_netconf_driver_from_device`)

**Root cause:** The Rust model defines `HostKeyPolicy::TOFU` / `KnownHostsFile` / `PinnedKey` in `DeviceInfo`, and `DeviceRegistrationService` accepts it via `RegisterDeviceRequest`. However `device_ref_from_info()` — the mapper that converts Rust `DeviceInfo` to the protobuf `DeviceRef` sent to the Python adapter — does NOT include `host_key_policy`. On the Python side, `_netconf_driver_from_device` hardcodes `hostkey_verify=False`.

**Impact:** All NETCONF connections use `hostkey_verify=False` regardless of configured policy. No TOFU enforcement, no known-hosts checking.

**Fix direction:** Add `host_key_policy` to the `DeviceRef` protobuf message, populate it in `device_ref_from_info()`, consume in `_netconf_driver_from_device`.

---

## LOW — Rust DeviceConfigRenderer Dead Code

**Files:** `src/device/render.rs` (all 99 lines)

**Finding:** `CiscoRenderer`, `H3cRenderer`, `HuaweiRenderer`, `RuijieRenderer` all implement `DeviceConfigRenderer` by returning `Err(UnderlayError::UnsupportedOperation)`. `grep -rn` across `src/` confirms zero callers outside `render.rs` and `mod.rs` (the re-export). `renderer_for_vendor()` constructs dead objects. Actual rendering is Python-side via gRPC.

**Fix direction:** Remove the four renderer structs, the trait, and `renderer_for_vendor()`, or wire them to the Python adapter.

---

## LOW — _admin_state_to_text: Three Implementations, Two Behaviors

**Files:** `backends/netconf.py:810-815`, `backends/mock_netconf.py:510-515`, `renderers/common.py:189-194`

**Finding:** Three copies of `_admin_state_to_text` exist with different behaviors:
- `netconf.py`: `int(value or 0) == 2` → "down", else "up" (UNSPECIFIED=0 → "up")
- `mock_netconf.py`: exact match only, else `str(value).lower()` (UNSPECIFIED=0 → "0")
- `renderers/common.py`: same pattern as mock_netconf.py

Currently unreachable in production because `interface_to_proto` only maps `Up(1)`/`Down(2)`. Test/mock fidelity gap only.

**Fix direction:** Consolidate to a single shared implementation.

---

## LOW — Five Python Driver Stubs That Crash on Construction

**Files:** `drivers/ruijie.py:5`, `drivers/legacy_cli.py:5`, `drivers/h3c.py:5`, `drivers/cisco.py:5`, `drivers/huawei.py:5`

**Finding:** Each class `__init__` immediately raises `NotImplementedError`. Any code path that instantiates them will panic at runtime rather than getting a clear "unsupported vendor" error.

**Fix direction:** Move the error to the driver factory so unsupported vendors fail at lookup time.

---

## LOW — SmallFabric Topology Skips Endpoint Count Validation

**Files:** `src/intent/validation.rs:129-144`

**Finding:** `validate_topology_shape` checks `StackSingleManagementIp` (requires exactly 1 endpoint) and `MlagDualManagementIp` (requires exactly 2 endpoints), but `SmallFabric` falls through to `_ => Ok(())` with no validation. Any number of endpoints passes.

**Fix direction:** Add minimum endpoint count validation for SmallFabric, or document the intended range.

---

## LOW — Jitter Source Not Independently Random

**Files:** `src/tx/endpoint_lock.rs:68-75`

**Finding:** Jitter uses `SystemTime::now().subsec_nanos()` directly. Concurrent callers within the same sub-second window get correlated jitter values, slightly reducing thundering-herd protection. Exponential backoff still provides primary protection. Low practical impact.

**Fix direction:** Use a proper PRNG (e.g., `rand::thread_rng()`) for independent jitter values.

---

## LOW — _normalized_scope_vlan_ids Defensive Gap

**Files:** `backends/netconf.py:649`, `renderers/common.py:105`

**Finding:** `int(vlan_id)` in set comprehensions without try/except. Protobuf uint32 prevents non-numeric values, so no current trigger. Defensive hardening only.

**Fix direction:** Add explicit error message if `int()` conversion fails.
