# Sprint 2C Fixture Parser Driver Integration Design

## Goal

Exercise the fixture-verified Huawei/H3C running-state parsers through the NETCONF-backed driver and backend, without making fixture parsers production-ready or depending on live switches.

## Scope

This phase proves the adapter can read fixture XML from a NETCONF session, parse it into the standard observed-state shape, expose that shape through `GetCurrentState`, and use it for `Verify`.

The default production path stays fail-closed:

- `NetconfBackedDriver` without an explicit fixture flag still rejects Huawei/H3C parser selection before any device read.
- unknown vendors still return `STATE_PARSER_VENDOR_UNSUPPORTED`.
- fixture-verified parsers remain `production_ready=False`.

## Driver Policy

Add an opt-in constructor flag on `NetconfBackedDriver` for tests and offline fixture validation:

```python
NetconfBackedDriver(backend, allow_fixture_verified_parser=True)
```

When the flag is false, driver parser injection keeps using the default registry gate. When true, driver parser injection calls `state_parser_for_vendor(..., allow_fixture_verified=True)`.

This keeps production behavior conservative while allowing integration tests to cover the real parser code path through the same driver/backend boundary.

## Test Coverage

Add focused tests under `adapter-python/tests/test_netconf_backend.py`:

- driver `GetCurrentState` returns normalized Huawei fixture state.
- driver `Verify` succeeds when desired state matches fixture state.
- driver `Verify` returns `VERIFY_FAILED` when desired differs from fixture state.
- scoped VLAN/interface reads produce scoped observed state and scoped NETCONF filter.
- empty scope returns an empty state and performs no device read.
- default driver path still rejects fixture parser selection before device read.

## Documentation

Update project progress docs to mark Sprint 2C as local fixture integration complete, while clearly stating that fixture-verified is not production-ready and real device XML validation remains open.
