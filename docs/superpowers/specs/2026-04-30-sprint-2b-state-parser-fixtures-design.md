# Sprint 2B State Parser Fixtures Design

## Goal

Build a fixture-verified minimum NETCONF running-state parser path for Huawei VRP8 and H3C Comware7 without depending on live switches.

## Scope

This phase parses controlled XML fixtures into the adapter's standard observed-state dictionary:

```python
{
    "vlans": [
        {"vlan_id": 100, "name": "prod", "description": "production vlan"}
    ],
    "interfaces": [
        {
            "name": "GE1/0/1",
            "admin_state": "up",
            "description": "server uplink",
            "mode": {
                "kind": "access",
                "access_vlan": 100,
                "native_vlan": None,
                "allowed_vlans": [],
            },
        }
    ],
}
```

The parser remains not production-ready until real device XML and field behavior are captured. Fixture verification proves parser mechanics and fail-closed behavior only.

## Parser Model

Huawei and H3C keep vendor classes, but share a small common XML parser helper. Each parser has a profile with:

- vendor name
- profile name
- fixture verification flag
- production readiness flag
- XML namespaces

The common helper handles required text extraction, VLAN range validation, duplicate detection, and scope filtering.

## Fail-Closed Rules

Parsing raises `AdapterError` with `NETCONF_STATE_PARSE_FAILED` when fixture XML is malformed, required fields are missing, VLAN IDs are outside 1..4094, VLAN/interface names are duplicated, or port mode is unknown.

## Registry Policy

`state_parser_for_vendor()` continues to reject parser selection by default while `production_ready=False`. Tests may request `allow_fixture_verified=True` to select fixture-verified parsers. This preserves production fail-closed behavior while allowing CI to exercise real parser code.
