# Aria Underlay Adapter

Python southbound adapter for Aria Underlay.

The adapter is intentionally limited to device-facing work:

- capability probe
- NETCONF / NAPALM / Netmiko backends
- vendor driver translation
- device-level diff
- rollback artifacts

Rust owns global transaction semantics and final operation status.

Generate Python protobuf stubs after dependencies are installed:

```bash
python -m grpc_tools.protoc \
  -I ../proto \
  --python_out=aria_underlay_adapter/proto \
  --grpc_python_out=aria_underlay_adapter/proto \
  ../proto/aria_underlay_adapter.proto
```

## Offline state parser validation

Captured NETCONF running XML can be validated locally before a vendor parser is
promoted to production-ready:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml tests/fixtures/state_parsers/huawei/vrp8_running.xml
```

Scope the output to touched resources:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample-running.xml \
  --vlan 100 \
  --interface GE1/0/1
```

The command uses fixture-verified parsers only for offline sample qualification.
It does not change production driver behavior, and fixture verification is not
the same as `production_ready=True`.
