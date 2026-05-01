# Aria Underlay Adapter

Python southbound adapter for Aria Underlay.

The adapter is intentionally limited to device-facing work:

- capability probe
- NETCONF backend
- NAPALM / Netmiko backend plans, not implemented code paths
- vendor driver translation
- vendor renderer and running state parser selection
- device-level prepare / commit / rollback / verify
- offline parser and renderer qualification tools

Rust owns global transaction semantics and final operation status.

## Current readiness

Implemented and CI-covered:

- fake and mock drivers for local integration tests
- ncclient-based NETCONF backend
- fail-closed renderer and state-parser registries
- TOFU known-host trust store and known-hosts policy wiring
- dry-run candidate rendering preflight without opening a device session
- offline `aria-underlay-state-parse`
- offline `aria-underlay-render-snapshot`

Not production-ready yet:

- Huawei/H3C renderers are still skeleton renderers and are rejected by real
  NETCONF prepare unless explicitly marked `production_ready=True` after review.
- Huawei/H3C state parsers are fixture-verified only. They need real-device
  running XML samples before production use.
- Cisco/Ruijie renderers and state parsers are not implemented.
- NAPALM and Netmiko backends are not implemented.

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

Print a compact field summary while checking captured samples:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample-running.xml \
  --summary
```

Pretty-print successful JSON output for manual inspection:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample-running.xml \
  --pretty
```

Validate a batch of redacted samples with a manifest:

```json
{
  "samples": [
    {
      "name": "huawei-vrp8-lab",
      "vendor": "huawei",
      "xml": "huawei/vrp8/sample.redacted.xml",
      "scope": {
        "vlans": [100],
        "interfaces": ["GE1/0/1"]
      }
    },
    {
      "name": "h3c-comware7-lab",
      "vendor": "h3c",
      "xml": "h3c/comware7/sample.redacted.xml"
    }
  ]
}
```

```bash
aria-underlay-state-parse --manifest samples.json --pretty
```

Manifest XML paths can be absolute or relative to the manifest file. The
command exits `0` only when every sample passes, and exits `1` if any sample
fails while still printing a full batch report.

The command uses fixture-verified parsers only for offline sample qualification.
It does not change production driver behavior, and fixture verification is not
the same as `production_ready=True`.

## Offline renderer snapshot validation

Desired-state JSON can be rendered through skeleton Huawei/H3C renderers without
connecting to a device:

```json
{
  "vlans": [
    {"vlan_id": 100, "name": "prod", "description": "production vlan"}
  ],
  "interfaces": [
    {
      "name": "GE1/0/1",
      "admin_state": "up",
      "description": "server uplink",
      "mode": {"kind": "access", "access_vlan": 100}
    }
  ]
}
```

```bash
aria-underlay-render-snapshot \
  --vendor huawei \
  --desired-state desired-state.json \
  --pretty
```

The command prints a JSON report with renderer profile metadata, resource
counts, and the rendered edit-config XML. It intentionally uses skeleton
renderers for offline snapshot qualification only. Production NETCONF prepare
still rejects skeleton renderers unless they are separately reviewed and marked
`production_ready=True`.
