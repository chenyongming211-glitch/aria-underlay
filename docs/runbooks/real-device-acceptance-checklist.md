# Real Device Acceptance Checklist

Copy this checklist for every switch/model acceptance run.

## Identification

- [ ] Date:
- [ ] Operator:
- [ ] Repository commit SHA:
- [ ] Adapter image:
- [ ] Probe binary source or artifact:
- [ ] Device IP:
- [ ] Device model:
- [ ] Secret ref:
- [ ] Test VLAN:

## Write-Before

- [ ] Adapter service is active.
- [ ] Adapter listens on loopback or an approved secure channel.
- [ ] NETCONF TCP port is reachable from the control node.
- [ ] Test VLAN is absent before write.
- [ ] Access port is approved for temporary PVID change.
- [ ] Access original PVID is recorded.
- [ ] Trunk port is approved for temporary allowed VLAN change.
- [ ] Trunk original allowed VLAN list is recorded exactly.
- [ ] Environment file contains no password or private key material.
- [ ] `real_domain_apply_probe` is available and executable.

## Access Dry-Run

- [ ] Dry-run contains `CreateVlan` for the test VLAN.
- [ ] Dry-run contains `UpdateInterface` for the access port.
- [ ] Dry-run contains no `DeleteVlan`.
- [ ] Dry-run contains no `DeleteInterfaceConfig`.

## Access Apply

- [ ] Apply returned `Success` or `SuccessWithWarning`.
- [ ] Transaction strategy is recorded.
- [ ] `tx_id` is recorded.
- [ ] Readback shows test VLAN exists.
- [ ] Readback shows access port PVID is the test VLAN.

## Access Cleanup

- [ ] Cleanup dry-run payload was inspected.
- [ ] Cleanup executed with `--yes`.
- [ ] Cleanup readback shows test VLAN absent.
- [ ] Cleanup readback shows access port restored.

## Trunk Dry-Run

- [ ] Dry-run contains `CreateVlan` for the test VLAN.
- [ ] Dry-run contains `UpdateInterface` for the trunk port.
- [ ] Dry-run before allowed VLAN list matches the recorded original list.
- [ ] Dry-run after allowed VLAN list includes the test VLAN.
- [ ] Dry-run contains no `DeleteVlan`.
- [ ] Dry-run contains no `DeleteInterfaceConfig`.

## Trunk Apply

- [ ] Apply returned `Success` or `SuccessWithWarning`.
- [ ] Transaction strategy is recorded.
- [ ] `tx_id` is recorded.
- [ ] Readback shows test VLAN exists.
- [ ] Readback shows trunk allowed VLAN list matches the requested test list.

## Trunk Cleanup

- [ ] Cleanup dry-run payload was inspected.
- [ ] Cleanup executed with `--yes`.
- [ ] Cleanup readback shows test VLAN absent.
- [ ] Cleanup readback shows trunk allowed VLAN list restored exactly.

## Closeout

- [ ] Adapter logs were checked for unexpected errors.
- [ ] No recoverable transaction remains for the tested device.
- [ ] The completed record is stored with release or lab notes.
- [ ] Any skipped case has a written reason.
