# Sprint 2G Validator Manifest Design

## Goal

Make `aria-underlay-state-parse` useful for validating a batch of redacted NETCONF running XML samples after field capture.

## Scope

This phase extends the offline validator only. It does not connect to devices, does not change `NetconfBackedDriver`, and does not make fixture parsers production-ready.

Add one option:

- `--manifest`: read a JSON manifest that lists samples to validate.

`--manifest` is mutually exclusive with single-sample `--vendor` and `--xml`. Existing single-sample behavior remains unchanged.

## Manifest Format

The manifest is a JSON object with a `samples` array:

```json
{
  "samples": [
    {
      "name": "huawei-vrp8-fixture",
      "vendor": "huawei",
      "xml": "adapter-python/tests/fixtures/state_parsers/huawei/vrp8_running.xml",
      "scope": {
        "vlans": [100],
        "interfaces": ["GE1/0/1"]
      }
    }
  ]
}
```

`scope` is optional. Without scope, a sample is parsed as full observed state.

Relative XML paths are resolved relative to the manifest file location. This keeps a real sample manifest portable inside the fixture tree.

## Output

The validator prints one JSON report to stdout:

```json
{
  "ok": false,
  "sample_count": 2,
  "passed": 1,
  "failed": 1,
  "samples": [
    {
      "name": "sample-a",
      "ok": true,
      "summary": {
        "vendor": "huawei",
        "profile_name": "vrp8-state-fixture",
        "fixture_verified": true,
        "production_ready": false,
        "vlan_count": 1,
        "interface_count": 1,
        "scope": {
          "full": false,
          "vlan_ids": [100],
          "interface_names": ["GE1/0/1"]
        }
      }
    },
    {
      "name": "sample-b",
      "ok": false,
      "error": {
        "code": "NETCONF_STATE_PARSE_FAILED",
        "message": "failed to parse NETCONF running state",
        "normalized_error": "parser_error",
        "raw_error_summary": "missing required text: vlan/vlan-id",
        "retryable": false
      }
    }
  ]
}
```

The command exits `0` only when every sample passes. It exits `1` when any sample fails, but still prints the full batch report to stdout so operators can see every sample result.

## Error Handling

Manifest validation failures return `1` and print structured JSON to stderr. Examples include invalid JSON, missing `samples`, non-object sample entries, missing sample fields, non-list scope fields, invalid scope item types, and mixing `--manifest` with single-sample arguments.

Per-sample parser failures and unreadable XML files are captured inside the stdout batch report instead of aborting the whole batch. This lets one bad sample coexist with successful samples in the same triage run.

## Tests

Add validator tests for:

- a manifest with two successful samples;
- a manifest with one successful sample and one parser failure;
- relative XML paths resolved from the manifest location;
- invalid manifest shape returns structured stderr JSON.

## Production Boundary

Manifest validation is still sample qualification. A successful batch is evidence for parser development and regression coverage, not permission to set `production_ready=True`.
