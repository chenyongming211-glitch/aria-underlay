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
- [ ] Test VLAN description:
- [ ] Test ACL:
- [ ] Test ACL description:

## Write-Before

- [ ] Adapter service is active.
- [ ] Adapter listens on loopback or an approved secure channel.
- [ ] NETCONF TCP port is reachable from the control node.
- [ ] Test VLAN is absent before write.
- [ ] Access port is approved for temporary PVID change.
- [ ] Access original PVID is recorded.
- [ ] Access original description is recorded exactly, including empty value.
- [ ] Trunk port is approved for temporary allowed VLAN change.
- [ ] Trunk original allowed VLAN list is recorded exactly.
- [ ] Trunk original description is recorded exactly, including empty value.
- [ ] Existing IPv4 advanced ACL ids are recorded.
- [ ] Test ACL id is absent before write.
- [ ] Test ACL id was re-checked immediately before write.
- [ ] Environment file contains no password or private key material.
- [ ] `real_domain_apply_probe` is available and executable.

## Access Dry-Run

- [ ] Dry-run contains `CreateVlan` for the test VLAN.
- [ ] Dry-run contains `UpdateInterface` for the access port.
- [ ] Dry-run includes the expected access/VLAN descriptions when configured.
- [ ] Dry-run contains no `DeleteVlan`.
- [ ] Dry-run contains no `DeleteInterfaceConfig`.

## Access Apply

- [ ] Apply returned `Success` or `SuccessWithWarning`.
- [ ] Transaction strategy is recorded.
- [ ] `tx_id` is recorded.
- [ ] Readback shows test VLAN exists.
- [ ] Readback shows test VLAN description when configured.
- [ ] Readback shows access port PVID is the test VLAN.
- [ ] Readback shows access port description when configured.

## Access Cleanup

- [ ] Cleanup dry-run payload was inspected.
- [ ] If clearing an empty original description, cleanup dry-run showed the SSH CLI `undo description` sequence.
- [ ] Cleanup executed with `--yes`.
- [ ] Cleanup readback shows test VLAN absent.
- [ ] Cleanup readback shows access port restored.
- [ ] Cleanup readback shows access description restored or cleared.

## Trunk Dry-Run

- [ ] Dry-run contains `CreateVlan` for the test VLAN.
- [ ] Dry-run contains `UpdateInterface` for the trunk port.
- [ ] Dry-run before allowed VLAN list matches the recorded original list.
- [ ] Dry-run after allowed VLAN list includes the test VLAN.
- [ ] Dry-run includes the expected trunk/VLAN descriptions when configured.
- [ ] Dry-run contains no `DeleteVlan`.
- [ ] Dry-run contains no `DeleteInterfaceConfig`.

## Trunk Apply

- [ ] Apply returned `Success` or `SuccessWithWarning`.
- [ ] Transaction strategy is recorded.
- [ ] `tx_id` is recorded.
- [ ] Readback shows test VLAN exists.
- [ ] Readback shows test VLAN description when configured.
- [ ] Readback shows trunk allowed VLAN list matches the requested test list.
- [ ] Readback shows trunk port description when configured.

## Trunk Cleanup

- [ ] Cleanup dry-run payload was inspected.
- [ ] If clearing an empty original description, cleanup dry-run showed the SSH CLI `undo description` sequence.
- [ ] Cleanup executed with `--yes`.
- [ ] Cleanup readback shows test VLAN absent.
- [ ] Cleanup readback shows trunk allowed VLAN list restored exactly.
- [ ] Cleanup readback shows trunk description restored or cleared.

## ACL Dry-Run

- [ ] Access/trunk variables are unset unless intentionally testing them too.
- [ ] `ARIA_UNDERLAY_TEST_VLAN` is unset unless intentionally testing VLAN too.
- [ ] Dry-run contains `CreateAcl` for the absent test ACL id.
- [ ] Dry-run contains no `UpdateAcl`.
- [ ] Dry-run contains no `DeleteAcl`.

## ACL Apply

- [ ] Apply returned `Success` or `SuccessWithWarning`.
- [ ] Transaction strategy is recorded.
- [ ] `tx_id` is recorded.
- [ ] Readback shows test ACL exists.
- [ ] Readback shows test ACL description when configured.
- [ ] Readback shows ACL rule fields match requested values.
- [ ] Readback confirms no ACL binding was added.

## ACL Cleanup

- [ ] Cleanup dry-run payload was inspected.
- [ ] Cleanup executed with `--yes`.
- [ ] Cleanup readback shows test ACL absent.

## Closeout

- [ ] Adapter logs were checked for unexpected errors.
- [ ] No recoverable transaction remains for the tested device.
- [ ] No test ACL remains.
- [ ] The completed record is stored with release or lab notes.
- [ ] Any skipped case has a written reason.
