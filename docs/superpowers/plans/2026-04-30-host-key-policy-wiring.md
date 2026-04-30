# HostKeyPolicy Wiring Plan

Date: 2026-04-30

## Goal

Fix the P1 finding where Rust `DeviceInfo.host_key_policy` was dropped before
the Python NETCONF backend, causing every real NETCONF connection to use
`hostkey_verify=False`.

## Scope

1. Add host key policy fields to `DeviceRef` in the adapter protobuf.
2. Map Rust `HostKeyPolicy` into the protobuf in `device_ref_from_info()`.
3. Consume the protobuf policy in the Python adapter server.
4. Carry policy details into `NcclientNetconfBackend`.
5. Fail closed for policies the backend cannot honestly enforce.
6. Add regression tests at Rust mapper and Python server/backend boundaries.

## Design Decisions

- `KnownHostsFile` maps to `hostkey_verify=True` plus an ncclient SSH config
  shim that sets `UserKnownHostsFile`.
- `TrustOnFirstUse` maps to `hostkey_verify=True`; it no longer silently disables
  verification. Full first-use persistence still needs a durable trust store.
- `PinnedKey { fingerprint }` is transported end to end, but NETCONF session
  opening fails closed until exact fingerprint pinning is implemented. ncclient
  supports `hostkey_b64` exact-key pinning, not the fingerprint-only shape
  currently stored by the Rust model.

## Verification

- `python3 -m pytest -q adapter-python/tests/test_server_renderer_selection.py adapter-python/tests/test_netconf_backend.py::test_ncclient_backend_rejects_pinned_fingerprint_without_exact_pin_support`
- Rust mapper coverage is added in `tests/adapter_mapper_tests.rs`; local Rust
  execution depends on GitHub Actions because this workstation has no `cargo`.
