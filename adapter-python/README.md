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

