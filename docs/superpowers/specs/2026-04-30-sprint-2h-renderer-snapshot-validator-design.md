# Sprint 2H Renderer Snapshot Validator Design

## Goal

Make Huawei/H3C renderer skeleton output easy to validate offline without a real switch.

## Scope

This phase adds an offline renderer snapshot tool only. It does not connect to devices, does not change `NetconfBackedDriver`, and does not make skeleton renderers production-ready.

Add one console script:

- `aria-underlay-render-snapshot`

The command reads a desired-state JSON file, selects a vendor renderer with `allow_skeleton=True`, renders edit-config XML, and prints a JSON snapshot report.

## Input Format

The desired-state input is a JSON object:

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

This mirrors the renderer-facing shape already covered in tests. It intentionally avoids protobuf generation requirements for field triage.

## Output

Successful output is JSON:

```json
{
  "vendor": "huawei",
  "profile_name": "vrp8-skeleton",
  "production_ready": false,
  "vlan_count": 1,
  "interface_count": 1,
  "xml": "<config>...</config>"
}
```

`--pretty` pretty-prints the JSON output. Errors are compact JSON on stderr and return exit code `1`.

## Error Handling

The command must fail closed for:

- unsupported vendors;
- malformed desired-state JSON;
- non-object desired-state payloads;
- empty desired state;
- renderer validation errors such as invalid VLAN ID, empty interface name, duplicate trunk VLANs, or unknown port mode.

Renderer validation errors are mapped to structured adapter-style JSON with code `RENDER_SNAPSHOT_FAILED`.

## Production Boundary

The tool is offline-only and explicitly uses skeleton renderers for snapshot qualification. A successful snapshot is evidence for renderer development, not permission to enable production rendering.
