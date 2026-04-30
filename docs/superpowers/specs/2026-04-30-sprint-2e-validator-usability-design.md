# Sprint 2E Validator Usability Design

## Goal

Make `aria-underlay-state-parse` useful during field sample triage by adding human-readable formatting and a compact parser/state summary.

## Scope

This phase extends the existing offline validator only. It does not connect to devices, does not change `NetconfBackedDriver`, and does not make fixture parsers production-ready.

Add two options:

- `--pretty`: pretty-print JSON with stable indentation.
- `--summary`: output a compact JSON summary instead of the full observed state.

The summary includes:

- `vendor`
- `profile_name`
- `fixture_verified`
- `production_ready`
- `vlan_count`
- `interface_count`
- `scope`

## Behavior

Default behavior remains unchanged: successful parsing prints compact observed-state JSON to stdout.

`--pretty` applies to whichever JSON payload is selected.

`--summary` prints parser and resource metadata so operators can quickly see whether the sample was parsed by the expected profile and how many resources were found.

Errors continue to return non-zero and emit compact JSON to stderr. Error output remains machine-readable and is not affected by `--pretty`, because failure handling is usually consumed by scripts or CI.

## Tests

Add validator tests for:

- pretty observed-state JSON output.
- summary JSON output with parser profile and resource counts.
- scoped summary JSON reflects requested VLAN/interface scope.

## Production Boundary

The validator remains a sample qualification tool. A successful summary is evidence for parser development, not permission to set `production_ready=True`.
