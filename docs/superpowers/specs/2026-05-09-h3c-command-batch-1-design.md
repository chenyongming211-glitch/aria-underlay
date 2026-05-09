# H3C Command Batch 1 Design

## Goal

Complete the low-risk H3C command surface before moving into QoS, PBR, NQA, or
BGP.

## Scope

Batch 1 has two parts:

- Implement ACL rule description closure.
- Define explicit delete-intent boundaries for a later implementation.

## ACL Rule Description

The Rust model and proto already expose `AclRule.description`. H3C support is
currently incomplete because the H3C renderer and parser do not round-trip the
field.

The implementation will:

- Render a non-empty ACL rule description as `IPv4AdvanceRules/Rule/Description`.
- Parse `IPv4AdvanceRules/Rule/Description` into normalized state.
- Reuse existing ACL verification, which already compares normalized rule
  descriptions.
- Add `ARIA_UNDERLAY_ACL_RULE_DESCRIPTION` to the real-device probe and
  acceptance documentation.

The implementation will not add ACL rule deletion, rule reordering semantics,
or ACL family expansion.

## Explicit Delete Intent

Deletes must never be inferred from missing desired state in merge/upsert mode.
This is the main production safety boundary.

The next delete implementation should add explicit delete requests for isolated
objects only. Dry-run must list exact delete operations and real-device probes
must require a separate acknowledgement for delete tests.

Initial delete candidates:

- ACL binding delete by interface, direction, and ACL id.
- ACL delete by id.
- VLAN delete by id.

Execution order must protect references:

- Unbind ACLs before deleting ACL objects.
- Detach future policies before deleting referenced ACLs or policy objects.
- Restore test ports before deleting test VLANs.

## Testing

ACL rule description will be covered by:

- H3C renderer test asserting rule `Description` is emitted.
- H3C parser test asserting rule `Description` is read.
- Existing verify tests through normalized ACL rule comparison.
- Real-device acceptance documentation and env example updates.

Delete intent is documentation-only in this batch. It gets implementation tests
when its explicit request schema is added.

## Safety

Real-device ACL description tests must use an ACL id proven absent by live
readback. The ACL must remain isolated unless the ACL binding case is explicitly
being tested. Cleanup deletes only the isolated test ACL after readback.
