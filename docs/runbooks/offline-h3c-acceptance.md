# Offline H3C Acceptance Runner

This runner gives CI a repeatable H3C command-surface acceptance signal when no
real switch is available. Each scenario now runs parser-in-the-loop:

```text
desired state -> H3C renderer XML -> mock NETCONF apply -> H3C readback XML
  -> H3C state parser -> verify parsed state
```

It does not replace the real-device acceptance runbook. It verifies that the
current H3C renderer and mock NETCONF backend can exercise the supported command
surface end to end:

- VLAN create and VLAN description
- access interface mode and interface description
- trunk allowed VLANs
- IPv4 advanced ACL rules
- IPv4 basic ACL rules
- ACL rule description
- ACL interface binding
- explicit delete VLAN, interface config, ACL, and ACL binding cleanup intents

Run locally from the repository root after installing the adapter package:

```bash
python -m pip install -e "adapter-python[test]"
aria-underlay-h3c-offline-acceptance --pretty
```

The command prints a machine-readable JSON report to stdout and a human-readable
summary to stderr. Each scenario includes rendered XML size, generated readback
XML size, parser profile, parsed-vs-observed resource counts, and a
`change_plan` block. CI also writes both forms to an artifact:

```bash
aria-underlay-h3c-offline-acceptance \
  --pretty \
  --json-report report.json \
  --summary summary.txt
```

Acceptance passes only when every scenario renders valid H3C Comware XML,
completes mock NETCONF dry-run, prepare, commit, and final-confirm, emits H3C
Comware-like running XML, parses it with `H3cStateParser`, and verifies parsed
state against the post-apply scoped state.

Each scenario reports `change_plan` metadata as the pre-change safety surface
used before higher-risk features such as PBR and BGP:

- `stages`: dependency-ordered execution phases.
- `dependency_edges`: resource ordering constraints such as ACL binding before
  ACL delete.
- `blast_radius`: local VLAN/interface or policy reference impact.
- `rollback_order`: human-readable reverse order for cleanup and recovery
  review.

The report also includes `read_only_audits` for high-risk surfaces that must not
enter the write path yet. The current PBR/BGP audit is parser-only: it detects
PBR/BGP nodes in H3C running XML, reports structured `touched_scope` with
affected VRFs, BGP AS numbers, neighbors, route-policy references, PBR policy
references, ACL references, interfaces, raw XML paths, and warnings. BGP audit
also includes `neighbor_details` with local AS, neighbor address, remote AS,
session state, import/export policy, VRF, and the raw XML path for each parsed
neighbor. The report returns `write_decision=read_only` with unsupported paths
until real path-level write evidence exists.

## PBR/BGP Real-Sample Calibration

When there is no live switch environment, the runner can still calibrate the
PBR/BGP parser against redacted real running XML samples:

```bash
cd adapter-python
python -m aria_underlay_adapter.acceptance.offline_h3c \
  --pbr-bgp-sample-dir tests/fixtures/state_parsers/real_samples/h3c/comware7 \
  --pretty
```

The default sample directory is
`adapter-python/tests/fixtures/state_parsers/real_samples/h3c/comware7`. Missing
directories and empty directories are non-fatal, so CI stays green until real
samples are available. Once `*.xml` files exist, each sample is parsed and
reported under `real_sample_audits` with:

- `sample_path` and `sample_source`.
- `features_present`.
- `write_decision`.
- structured `touched_scope`.
- structured BGP `neighbor_details` when BGP nodes are present.
- `unsupported_paths`.
- `warnings`.

Invalid XML or parser errors fail the report for that sample. Passing sample
audits are parser calibration evidence only; they do not enable PBR/BGP writes
and do not replace real-device path-level profile verification.
