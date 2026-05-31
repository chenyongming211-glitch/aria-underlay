# Real Device Acceptance Runbook

## Scope

This runbook turns the H3C real-switch validation flow into a repeatable
acceptance procedure. The current supported acceptance surface is:

- VLAN create/update through NETCONF running edit-config.
- VLAN description update.
- Access port PVID update.
- Trunk port allowed VLAN update.
- Access/trunk interface description update.
- Isolated numeric IPv4 advanced ACL create/read/verify.
- IPv4 advanced ACL rule description.
- Interface packet-filter binding for an isolated numeric IPv4 advanced ACL.
- Scoped get-current-state readback and verify.

The base VLAN/access/trunk/ACL procedure has been exercised against H3C S5560
and S6800 representatives; newly documented fields still require the same
write/readback/cleanup loop before being marked accepted for a specific model.
It is intentionally scoped; do not use it as proof for admin-down, trunk native
VLAN, deletes through normal apply, PBR/QoS/NQA/BGP ACL consumers, or
cross-device atomic behavior.

## High-Risk Model Profile Requirement

Before adding real-device write acceptance for PBR, QoS policy binding, NQA
coupling, or BGP, capture a model profile for the exact device model and
firmware under test. The profile is required even when the final write path uses
H3C NETCONF instead of gNMI.

Required evidence:

- NETCONF server capabilities and YANG Library module/revision inventory.
- gNMI Capabilities output when gNMI is enabled or being evaluated.
- OpenConfig path read/write result for each target feature path, if present.
- Vendor native YANG path read/write result when OpenConfig is unavailable or
  incomplete.
- Whether the device supports candidate, validate, confirmed-commit, or only
  writable-running with rollback-on-error.
- A dry-run ChangePlan containing dependency edges, ordered stages, rollback
  order, touched scope, blast radius, unsupported paths, and final write
  decision (`DryRunWriteDecision`: `allowed_standard_model`,
  `allowed_vendor_native`, `allowed_vendor_private`, `read_only`, or
  `rejected`).

Do not treat module presence as write support. If only module-level evidence is
available, record the feature as read-only or rejected for writes. For PBR/BGP,
running-only write support remains rejected unless a separate high-risk
exception is approved and recorded with the acceptance result.

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
| Test ACL | Optional IPv4 advanced ACL number absent before the test |
| Test ACL description | Optional temporary ACL description |
| Test ACL binding interface | Optional approved idle port with no existing binding in the chosen direction |
| Test ACL binding direction | `inbound` or `outbound` |

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
- If testing ACL, the candidate ACL number is absent before the write.
- If testing ACL, no existing ACL number is reused.
- If testing ACL binding, the binding interface is approved for temporary
  packet-filter binding.
- If testing ACL binding, the selected interface and direction have no existing
  IPv4 ACL binding before the write.

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

## ACL Acceptance

The ACL MVP creates only a numeric IPv4 advanced ACL object. It must not bind
the ACL to any interface, VLAN interface, PBR policy, QoS policy, or routing
feature during this run.

1. Read existing ACL ids from the switch before choosing a candidate.

Use NETCONF `top/ACL` readback, `display acl all`, or both. Record every
existing IPv4 advanced ACL id. Choose a test ACL id only if it is absent. The
recommended temporary range is `3998-3999`, but the actual candidate must come
from live readback, not from convention.

If every candidate in the approved temporary range already exists, stop.

2. Set only the ACL variables in the environment.

Unset access and trunk variables for this case. Do not set
`ARIA_UNDERLAY_TEST_VLAN` unless the same run is also intentionally testing
VLAN behavior.

```bash
set -a
. ./real-device-acceptance.env
set +a
unset ARIA_UNDERLAY_ACCESS_INTERFACE
unset ARIA_UNDERLAY_TRUNK_INTERFACE
unset ARIA_UNDERLAY_TEST_VLAN
export ARIA_UNDERLAY_TEST_ACL_ID=<absent-acl-id>
export ARIA_UNDERLAY_TEST_ACL_DESCRIPTION="aria isolated acl"
export ARIA_UNDERLAY_ACL_RULE_SEQUENCE=10
export ARIA_UNDERLAY_ACL_RULE_ACTION=permit
export ARIA_UNDERLAY_ACL_RULE_PROTOCOL=ip
export ARIA_UNDERLAY_ACL_RULE_SOURCE=192.0.2.1
export ARIA_UNDERLAY_ACL_RULE_SOURCE_WILDCARD=0.0.0.0
export ARIA_UNDERLAY_ACL_RULE_DESTINATION=198.51.100.0
export ARIA_UNDERLAY_ACL_RULE_DESTINATION_WILDCARD=0.0.0.255
export ARIA_UNDERLAY_ACL_RULE_DESCRIPTION="aria isolated acl rule"
```

3. Run the real apply probe.

```bash
/opt/aria-underlay/probes/real_domain_apply_probe
```

The dry-run must show `CreateAcl` for the chosen ACL id. If it shows
`UpdateAcl`, `DeleteAcl`, or no `CreateAcl` for the requested id, stop and
choose another absent ACL id.

4. Read back the ACL scope.

Acceptance requires:

- The test ACL exists.
- The test ACL description matches when configured.
- The test rule sequence, action, protocol, source, destination, and ports
  match the requested values.
- The test rule description matches when configured.
- No ACL binding has been added.

5. Clean up and verify again.

Run cleanup in dry-run first:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --delete-acl "$ARIA_UNDERLAY_TEST_ACL_ID" \
  --dry-run
```

Then execute:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --delete-acl "$ARIA_UNDERLAY_TEST_ACL_ID" \
  --yes
```

Read back the ACL scope again. The test ACL must be absent.

## ACL Binding Acceptance

This case proves the interface packet-filter binding boundary. It creates an
isolated ACL and binds it to one approved interface/direction. It does not
prove PBR, QoS traffic-classifier, NQA, BGP, or other ACL consumers.

1. Confirm the candidate ACL and binding target are clean.

- The test ACL id must be absent by live readback.
- The selected interface must be approved for temporary packet-filter binding.
- The selected direction must have no existing IPv4 ACL binding on that
  interface.

If any of these checks fail, choose a different absent ACL id or a different
approved idle port.

2. Set ACL and binding variables.

Unset access and trunk variables. Do not set `ARIA_UNDERLAY_TEST_VLAN` unless
the same run is intentionally testing VLAN behavior. The binding references an
existing interface by name; it does not require changing that interface's
access or trunk configuration.

```bash
set -a
. ./real-device-acceptance.env
set +a
unset ARIA_UNDERLAY_ACCESS_INTERFACE
unset ARIA_UNDERLAY_TRUNK_INTERFACE
unset ARIA_UNDERLAY_TEST_VLAN
export ARIA_UNDERLAY_TEST_ACL_ID=<absent-acl-id>
export ARIA_UNDERLAY_TEST_ACL_DESCRIPTION="aria isolated acl binding"
export ARIA_UNDERLAY_ACL_BIND_INTERFACE=<approved-idle-interface>
export ARIA_UNDERLAY_ACL_BIND_DIRECTION=inbound
unset ARIA_UNDERLAY_ACL_BIND_ID
```

When `ARIA_UNDERLAY_ACL_BIND_ID` is unset, the probe binds the first declared
test ACL. Set it only when the same run declares more than one temporary ACL.

3. Run the real apply probe.

```bash
/opt/aria-underlay/probes/real_domain_apply_probe
```

The dry-run must show `CreateAcl` for the chosen ACL id and
`CreateAclBinding` for the selected interface/direction. If it shows
`UpdateAcl`, `DeleteAcl`, `UpdateAclBinding`, `DeleteAclBinding`, or no
`CreateAclBinding` for the requested target, stop.

4. Read back the ACL and binding scope.

Acceptance requires:

- The test ACL exists and rules match the request.
- The selected interface/direction is bound to the test ACL id.
- No unrelated ACL binding appears in the scoped readback.

5. Clean up and verify again.

Run cleanup in dry-run first. The cleanup order is important: unbind first,
then delete the ACL.

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --unbind-acl-interface "$ARIA_UNDERLAY_ACL_BIND_INTERFACE" \
  --unbind-acl-direction "$ARIA_UNDERLAY_ACL_BIND_DIRECTION" \
  --unbind-acl-id "$ARIA_UNDERLAY_TEST_ACL_ID" \
  --delete-acl "$ARIA_UNDERLAY_TEST_ACL_ID" \
  --dry-run
```

Then execute:

```bash
python3 scripts/real_device_cleanup.py \
  --host "$ARIA_UNDERLAY_MGMT_IP" \
  --secret-ref "$ARIA_UNDERLAY_SECRET_REF" \
  --unbind-acl-interface "$ARIA_UNDERLAY_ACL_BIND_INTERFACE" \
  --unbind-acl-direction "$ARIA_UNDERLAY_ACL_BIND_DIRECTION" \
  --unbind-acl-id "$ARIA_UNDERLAY_TEST_ACL_ID" \
  --delete-acl "$ARIA_UNDERLAY_TEST_ACL_ID" \
  --yes
```

Read back the ACL and binding scope again. The binding must be absent and the
test ACL must be absent.

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
- If ACL dry-run contains `UpdateAcl` or `DeleteAcl`, stop. The candidate ACL
  is not a clean isolated create.
- If ACL binding dry-run contains `UpdateAclBinding` or `DeleteAclBinding`,
  stop. The selected interface/direction already has state that this run would
  disturb.
- If the apply fails before a `tx_id` is produced, collect adapter logs and the
  probe output; no transaction recovery action is expected.
- If the apply fails after a `tx_id` is produced, record the `tx_id`, strategy,
  error code, adapter logs, and readback state for the touched scope.
- If cleanup fails, do not start another acceptance run on the same resources.
  Restore the recorded original state first.
- If readback after cleanup still shows the test VLAN or test port state, keep
  the record open and treat the switch as manually dirty.
- If readback after cleanup still shows the test ACL binding, unbind it before
  deleting or reusing the ACL id.

## Exit Criteria

The acceptance run is complete only when:

- Access and trunk cases both pass for the representative switch model, or the
  skipped case is explicitly recorded.
- Every write has a readback proof.
- Every cleanup has a readback proof.
- No test VLAN remains.
- No test ACL remains.
- No test ACL binding remains.
- Every changed port is restored to its recorded original PVID/allowed VLAN and
  description state.
- The record template is filled in and stored with the release/test notes.
