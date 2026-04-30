# Bug Inventory — 2026-04-26

Comprehensive bug report from full codebase review (~10,500 lines, ~73% sprint completion). 11 bugs fixed, 32 remaining.

## Rust — High Severity (4)

- ~~**GC no-op**~~ — ✅ FIXED: `JournalGc::run_once` now prunes old terminal journal records by retention policy, never auto-deletes `InDoubt` or non-terminal records, and can clean terminal rollback artifacts with per-device retention caps.
- ~~**Weak intent validation**~~ — ✅ FIXED: `validate_switch_pair_intent` and `validate_underlay_domain_intent` now reject empty IDs, duplicate switches/endpoints/members/VLANs/interfaces, invalid VLAN ranges, undeclared VLAN references, empty endpoint credential refs, and topology/management endpoint shape mismatches.
- ~~**Journal write silently ignored**~~ — ✅ FIXED (`d71a4d4`): `let _ = self.journal.put(...)` replaced with proper `if let Err(...)` that includes the journal failure in the returned error message. Now at line 407 (not 378).
- ~~**Shadow write downgraded to warning**~~ — ✅ FIXED (`d71a4d4`): Shadow store write failure now returns early with `SuccessWithWarning` + explicit error_code instead of bare warning in a Success result. Now at line 382.

## Rust — Medium Severity (12)

- `src/api/service.rs:715-720` — Empty-devices record with AdapterRecover creates permanent InDoubt cycle (manual intervention → empty → InDoubt forever)
- `src/api/service.rs:1043-1052` — `aggregate_apply_status` labels partial failure as `SuccessWithWarning` (misleading aggregate status)
- `src/tx/lock_strategy.rs:9` + `src/tx/endpoint_lock.rs:65` — ~~`jitter: bool` field defined but never used in exponential backoff~~ ✅ FIXED (`3c5c7d3`): Jitter now applied as up to 25% randomized addition to backoff delay when `policy.jitter` is true.
- `src/tx/journal.rs:99` — `InMemoryTxJournalStore` uses `std::sync::Mutex` in async context (thread blocking)
- `src/device/bootstrap.rs:117-157` — Orphaned secrets when registration fails after secret creation, no cleanup
- `src/api/service.rs:178-181` — No gRPC connection pooling; new `AdapterClient::connect()` per operation (connection churn, fd exhaustion)
- `src/api/service.rs:722,870` — Recovery reads journal before lock, doesn't re-read after lock acquisition (potential duplicate recovery attempts)
- `src/api/service.rs:339-356` — Journal `Committed` written before shadow store update; crash leaves stale shadow
- `src/api/service.rs:880-908` — Recovery attempt history lost when transitioning to `InDoubt` (operator sees no prior-attempt context)
- ~~`src/tx/journal.rs:29-35` — `Failed` records are terminal but accumulate forever without GC~~ ✅ FIXED: `Failed` records are included in terminal GC with a separate `failed_journal_retention_days` policy.
- `src/adapter_client/mapper.rs:154-161` — ~~No 802.1Q VLAN ID validation (0, 4095, >4094 accepted)~~ ✅ FIXED (`3c5c7d3`): VLAN IDs outside 1–4094 now rejected at mapper boundary.
- `tests/recovery_tests.rs:290` + `tests/transaction_gate_tests.rs:260` — Fixed 50ms sleep for test server startup (TOCTOU race, CI flakiness)

## Rust — Low Severity (12)

- `src/adapter_client/mapper.rs:221-223` — `RecoveryAction::Noop` maps to `Unspecified` (intent loss, currently unreachable but misleading)
- `src/tx/endpoint_lock.rs:82-88` — `lock_for` takes `DeviceId` by value inconsistently (one caller clones, one doesn't)
- `src/engine/normalize.rs:40-46` — Whitespace-only interface names produce empty string
- ~~`src/device/bootstrap.rs:176-207` — Bootstrap validates pair shape but domain planner does not (architectural inconsistency)~~ ✅ FIXED: domain planner now calls shared `validate_underlay_domain_intent` before planning endpoint desired state.
- `src/adapter_client/mapper.rs:47-50,267-275` — Unknown backend enum values silently dropped via `filter_map`
- `src/tx/endpoint_lock.rs:17` — `_guards` field underscore implies unused but RAII is essential
- `src/adapter_client/mapper.rs:39` — `vendor_hint.unwrap_or(Unknown)` loses distinction between "no hint" and "known unknown"
- `src/adapter_client/mapper.rs:221-223` — `ManualIntervention` and `Noop` both map to `Unspecified` (currently safe because call site filters them out)
- `tests/transaction_tests.rs:55,93,112` — Test temp dir cleanup `.ok()` silently ignores errors
- `tests/transaction_gate_tests.rs:426` — `observed_state` test helper uses `u32` for VLAN IDs, hiding conversion boundary
- `tests/recovery_tests.rs:29-87` — Test relies on empty inventory side effect, fragile to refactoring
- `src/tx/candidate_commit.rs` + `src/tx/confirmed_commit.rs` + `src/tx/coordinator.rs` — Empty structs; all logic in `service.rs` (code organization)

## Python — High Severity (3)

- ~~`adapter-python/aria_underlay_adapter/artifact_store.py:10-15`~~ — ✅ FIXED (`3c5c7d3`): Path traversal blocked. `_root` is resolved at init; `save_json` resolves the target path and verifies it stays within `_root` before writing.
- ~~`adapter-python/aria_underlay_adapter/renderers/h3c.py:69-72` and `huawei.py:69-72`~~ — ✅ FIXED (`3c5c7d3`): Literal "None" string in XML. `_port_mode_element` now checks `access_vlan is None` and raises `ValueError` instead of passing `None` to `str()`.
- ~~`adapter-python/aria_underlay_adapter/backends/mock_netconf.py:425-428`~~ — ✅ FIXED: Full-scope verify now checks observed VLAN/interface IDs when `scope.full = true` or `scope is None`, so extra resources are detected instead of silently passing.

## Python — Medium Severity (8)

- `backends/mock_netconf.py:506-519` — `_normalize_mode` silently converts unknown port mode kinds (routed, hybrid) to "access"
- `backends/netconf.py:100-111` — `_discard_candidate` in exception handler can raise and swallow original error
- `backends/netconf.py:118-126,128-137,139-148,188-194,196-205` — Overly broad `except Exception` masks non-NETCONF errors (AttributeError, TypeError) as `NETCONF_*_FAILED`
- `backends/mock_netconf.py:473` vs renderers — `_optional_field` converts empty→None in mock but not in renderers (divergent behavior)
- `backends/mock_netconf.py:506-519` — `_normalize_mode` crashes with `TypeError` if mode is `None`
- `backends/mock_netconf.py:339-342` — `_is_confirmed_commit_strategy` accepts string values that real backend wouldn't (inconsistent with protobuf integer enums)
- `backends/netconf.py:375-404` — Error classification uses substring matching ("auth", "timeout"); fragile and prone to misclassification
- `drivers/netconf_backed.py:262-279` — `_port_mode_to_proto` passes `None` integer fields to protobuf, losing unset vs zero distinction

## Python — Low Severity (4)

- `backends/mock_netconf.py:456-459` — `_field` raises `KeyError` for dicts vs `AttributeError` for protobuf objects (different exception types for same logical condition)
- `drivers/base.py:42` — `DriverRegistry.select` raises bare `RuntimeError` instead of structured `AdapterError`
- `server.py:140-147` — `ForceUnlock` returns COMMITTED+changed=True when driver returns `None` (currently unreachable, but silent success if ever triggered)
- `tests/test_netconf_backend.py:260-263` — `_BackendWithSession` uses `object.__setattr__` to bypass frozen dataclass (fragile to `__slots__` addition)

## Status

11 bugs fixed (d71a4d4 + 3c5c7d3 + P0 mock verify fix + P0 intent validation fix + current P0 journal/artifact GC fix). 4 of 4 high Rust bugs fixed. 3 of 3 high Python bugs fixed. 32 remaining.
