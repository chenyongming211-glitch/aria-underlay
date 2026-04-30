# Sprint 2D State Parser Validator Design

## Goal

Add an offline validator for NETCONF running-state XML samples so Huawei/H3C real `get-config` captures can be checked locally before any parser is marked production-ready.

## Scope

This phase adds a Python adapter CLI entrypoint:

```bash
aria-underlay-state-parse --vendor huawei --xml sample.xml
```

The command reads XML from a file, selects a fixture-verified parser with `allow_fixture_verified=True`, parses the XML, and prints the normalized observed-state JSON shape used by the adapter.

The CLI is intentionally offline:

- it does not connect to a switch;
- it does not call `NetconfBackedDriver`;
- it does not change production parser selection;
- it does not mark parser profiles as production-ready.

## Behavior

Successful parsing prints deterministic JSON to stdout:

```json
{
  "interfaces": [],
  "vlans": []
}
```

The command supports optional scope flags:

- `--vlan 100`, repeatable.
- `--interface GE1/0/1`, repeatable.
- `--full`, default when no scope flags are provided.

Parser or registry failures return a non-zero exit code and print a compact JSON error to stderr containing the adapter error code and raw summary.

## Testing

Tests exercise the CLI through direct `main(argv)` calls:

- Huawei fixture parses and returns normalized JSON.
- scope flags filter VLAN/interface output.
- unsupported vendor fails with `STATE_PARSER_VENDOR_UNSUPPORTED`.
- invalid XML fails with `NETCONF_STATE_PARSE_FAILED`.

## Production Boundary

The validator is a sample qualification tool only. It gives the team a repeatable way to evaluate real XML captures, but parser promotion still requires separate evidence and a deliberate `production_ready=True` change.
