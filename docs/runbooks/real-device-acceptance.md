# Real Device Acceptance Runbook

## Scope

This runbook turns the H3C real-switch validation flow into a repeatable
acceptance procedure. The current production-verified surface is:

- VLAN create/update through NETCONF running edit-config.
- VLAN description update.
- Access port PVID update.
- Trunk port allowed VLAN update.
- Access/trunk interface description update.
- Scoped get-current-state readback and verify.

The procedure has been exercised against H3C S5560 and S6800 representatives.
It is intentionally scoped; do not use it as proof for interface descriptions,
admin-down, trunk native VLAN, deletes, or cross-device atomic behavior.

## Preconditions

- The Python adapter is running and healthy on the control node.
- The adapter listens on loopback, normally `127.0.0.1:50051`.
- The control node can reach the switch NETCONF SSH port, normally TCP 830.
- The switch secret is present in the adapter secret file.
- The Rust `real_domain_apply_probe` binary is available on the control node.
- The adapter image includes the H3C production renderer and scoped parser fixes.
- The test VLAN and test ports are approved for temporary changes.

Do not put switch passwords in this repository or in acceptance records. Record
only the `secret_ref`.

## Resource Selection

For each switch/model under test, record these values before writing:

| Field | Required value |
| --- | --- |
| Device IP | Management IP used by NETCONF |
| Model | Example: `S5560` or `S6800` |
| Adapter endpoint | Example: `http://127.0.0.1:50051` |
| Secret ref | Example: `lab/h3c` |
| Test VLAN | A VLAN that is absent before the test |
| Test VLAN description | Optional temporary description to verify |
| Access port | An approved idle access port |
| Access original PVID | Usually `1`, but verify first |
| Access original description | Exact text to restore, or explicit empty |
| Trunk port | An approved trunk port |
| Trunk original allowed VLANs | Exact list to restore after the test |
| Trunk original description | Exact text to restore, or explicit empty |

The acceptance VLAN used in previous lab runs was `4093`; this is only a
convention, not a requirement.

## Write-Before Checks

1. Confirm the adapter service and image.

```bash
systemctl is-active aria-underlay-adapter
docker ps --filter name=aria-underlay-adapter --format '{{.Names}} {{.Image}} {{.Status}}'
```

2. Confirm NETCONF reachability from the control node.

```bash
nc -vz <switch-ip> 830
```

3. Run a scoped read for the exact VLANs and ports you plan to touch.

The scoped read may be done with the adapter gRPC client, a local probe, or an
operator wrapper. The required evidence is:

- Test VLAN is absent before the write.
- Access port current PVID is recorded.
- Access port current description is recorded.
- Trunk port current allowed VLAN list is recorded.
- Trunk port current description is recorded.
- No unapproved interface appears in the scoped readback.

4. Prepare the environment file from
`docs/examples/real-device-acceptance.env.example`.

## Access Port Acceptance

1. Set only the access variables in the environment.

```bash
set -a
. ./real-device-acceptance.env
set +a
unset ARIA_UNDERLAY_TRUNK_INTERFACE
unset ARIA_UNDERLAY_TRUNK_ALLOWED_VLANS
```

2. Run the real apply probe.

```bash
/opt/aria-underlay/probes/real_domain_apply_probe
```

3. Check the probe output before considering the test valid.

- `real_apply_dry_run_noop=false`
- `real_apply_change_sets` contains `CreateVlan` for the test VLAN.
- `real_apply_change_sets` contains `UpdateInterface` for the access port.
- `real_apply_change_sets` contains no `DeleteVlan`.
- `real_apply_change_sets` contains no `DeleteInterfaceConfig`.
- `real_apply_status` is `Success` or `SuccessWithWarning`.
- `real_apply_strategy` is recorded.
- `tx_id` is recorded.

For H3C devices that only support writable-running with rollback-on-error, the
expected result is usually `SuccessWithWarning` with strategy
`RunningRollbackOnError`.

4. Read back the same scope.

Acceptance requires:

- The test VLAN exists with the expected name.
- The test VLAN has the expected description when
  `ARIA_UNDERLAY_TEST_VLAN_DESCRIPTION` is set.
- The access port reports access mode with the test VLAN as PVID.
- The access port has the expected description when
  `ARIA_UNDERLAY_ACCESS_DESCRIPTION` is set.

5. Clean up and verify again.

Run cleanup in dry-run first:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --access-interface "$ARIA_UNDERLAY_ACCESS_INTERFACE" \
  --access-pvid "$ARIA_UNDERLAY_ACCESS_ORIGINAL_PVID" \
  --description-interface "$ARIA_UNDERLAY_ACCESS_INTERFACE" \
  --description "$ARIA_UNDERLAY_ACCESS_ORIGINAL_DESCRIPTION" \
  --delete-vlan "$ARIA_UNDERLAY_TEST_VLAN" \
  --dry-run
```

Then execute:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --access-interface "$ARIA_UNDERLAY_ACCESS_INTERFACE" \
  --access-pvid "$ARIA_UNDERLAY_ACCESS_ORIGINAL_PVID" \
  --description-interface "$ARIA_UNDERLAY_ACCESS_INTERFACE" \
  --description "$ARIA_UNDERLAY_ACCESS_ORIGINAL_DESCRIPTION" \
  --delete-vlan "$ARIA_UNDERLAY_TEST_VLAN" \
  --yes
```

Read back the same scope again. The test VLAN must be absent, and the access
port must no longer show the test PVID or temporary description. If the
original description was empty, replace the cleanup `--description` argument
with `--clear-description`. On tested H3C Comware devices, clearing an
interface description uses SSH CLI `undo description`; the cleanup tool prints
the exact CLI sequence during dry-run and still requires `--yes` before writing.

## Trunk Port Acceptance

1. Set only the trunk variables in the environment.

```bash
set -a
. ./real-device-acceptance.env
set +a
unset ARIA_UNDERLAY_ACCESS_INTERFACE
```

`ARIA_UNDERLAY_TRUNK_ALLOWED_VLANS` must include both the original allowed VLANs
and the test VLAN. Keep `ARIA_UNDERLAY_TRUNK_ORIGINAL_ALLOWED_VLANS` as the
exact list to restore.

2. Run the real apply probe.

```bash
/opt/aria-underlay/probes/real_domain_apply_probe
```

3. Check the probe output.

- The dry-run change set contains no delete operations.
- The test VLAN is created.
- The trunk interface changes from the original allowed VLAN list to the list
  that includes the test VLAN.
- The requested descriptions appear in the desired state when the optional
  description environment variables are set.
- The apply result is `Success` or `SuccessWithWarning`.
- The `tx_id` and transaction strategy are recorded.

4. Read back the same scope.

Acceptance requires:

- The test VLAN exists.
- The test VLAN has the expected description when
  `ARIA_UNDERLAY_TEST_VLAN_DESCRIPTION` is set.
- The trunk port allowed VLAN list exactly matches the requested test list.
- The trunk port has the expected description when
  `ARIA_UNDERLAY_TRUNK_DESCRIPTION` is set.

5. Clean up and verify again.

Run cleanup in dry-run first:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --trunk-interface "$ARIA_UNDERLAY_TRUNK_INTERFACE" \
  --trunk-allowed-vlans "$ARIA_UNDERLAY_TRUNK_ORIGINAL_ALLOWED_VLANS" \
  --description-interface "$ARIA_UNDERLAY_TRUNK_INTERFACE" \
  --description "$ARIA_UNDERLAY_TRUNK_ORIGINAL_DESCRIPTION" \
  --delete-vlan "$ARIA_UNDERLAY_TEST_VLAN" \
  --dry-run
```

Then execute:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --trunk-interface "$ARIA_UNDERLAY_TRUNK_INTERFACE" \
  --trunk-allowed-vlans "$ARIA_UNDERLAY_TRUNK_ORIGINAL_ALLOWED_VLANS" \
  --description-interface "$ARIA_UNDERLAY_TRUNK_INTERFACE" \
  --description "$ARIA_UNDERLAY_TRUNK_ORIGINAL_DESCRIPTION" \
  --delete-vlan "$ARIA_UNDERLAY_TEST_VLAN" \
  --yes
```

Read back the same scope again. The test VLAN must be absent, and the trunk
allowed VLAN list and description must exactly match the original values. If
the original description was empty, replace the cleanup `--description`
argument with `--clear-description`. On tested H3C Comware devices, clearing an
interface description uses SSH CLI `undo description`; the cleanup tool prints
the exact CLI sequence during dry-run and still requires `--yes` before writing.

## Running Cleanup In The Adapter Container

If the host Python environment does not have `ncclient`, run cleanup through the
adapter image:

```bash
docker run -i --rm --network host \
  -v /opt/aria-underlay/current:/work \
  -v /etc/aria-underlay:/etc/aria-underlay:ro \
  -w /work \
  -e PYTHONPATH=/work/adapter-python:/work/adapter-python/aria_underlay_adapter/proto \
  <adapter-image> \
  python3 scripts/real_device_cleanup.py \
    --host "$ARIA_UNDERLAY_MGMT_IP" \
    --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
    --delete-vlan "$ARIA_UNDERLAY_TEST_VLAN" \
    --dry-run
```

Replace `<adapter-image>` with the image currently running in production or in
the acceptance environment. When `--clear-description` is used, the container
also needs SSH access to the switch management address, normally TCP 22.

## Failure Handling

- If dry-run contains `DeleteVlan` or `DeleteInterfaceConfig`, stop. The request
  is not scoped safely enough for real-device acceptance.
- If the apply fails before a `tx_id` is produced, collect adapter logs and the
  probe output; no transaction recovery action is expected.
- If the apply fails after a `tx_id` is produced, record the `tx_id`, strategy,
  error code, adapter logs, and readback state for the touched scope.
- If cleanup fails, do not start another acceptance run on the same resources.
  Restore the recorded original state first.
- If readback after cleanup still shows the test VLAN or test port state, keep
  the record open and treat the switch as manually dirty.

## Exit Criteria

The acceptance run is complete only when:

- Access and trunk cases both pass for the representative switch model, or the
  skipped case is explicitly recorded.
- Every write has a readback proof.
- Every cleanup has a readback proof.
- No test VLAN remains.
- Every changed port is restored to its recorded original PVID/allowed VLAN and
  description state.
- The record template is filled in and stored with the release/test notes.
