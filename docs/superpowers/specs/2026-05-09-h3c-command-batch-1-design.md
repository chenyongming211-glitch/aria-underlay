# H3C Command Batch 1 Design

## Goal

Complete the low-risk H3C command surface before moving into QoS, PBR, NQA, or
BGP.

## Scope

Batch 1 has two parts:

- Implement ACL rule description closure.
- Implement explicit delete-intent boundaries for isolated VLAN, ACL, and ACL
  binding targets.

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

The delete implementation adds explicit delete requests for isolated objects
only. Dry-run lists exact delete operations when the target exists in the scoped
read. The normal real-device apply probe still refuses delete plans, so delete
acceptance must use a dedicated cleanup/delete path.

Initial delete candidates:

- ACL binding delete by interface, direction, and ACL id.
- ACL delete by id.
- VLAN delete by id.

Schema:

- `UnderlayDomainIntent.delete_vlan_ids`
- `UnderlayDomainIntent.delete_acl_ids`
- `UnderlayDomainIntent.delete_acl_bindings`
- Matching `DeviceDesiredState` and protobuf fields for adapter handoff.

Apply behavior:

- Reconciliation has one explicit merge/upsert semantic.
- Deletes are allowed only for explicit targets.
- Full replacement by absence is unsupported.

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

Delete intent is covered by Rust diff/planner/mapper tests, Python H3C renderer
tests, mock backend merge behavior, and verify absence checks.

## Safety

Real-device ACL description tests must use an ACL id proven absent by live
readback. The ACL must remain isolated unless the ACL binding case is explicitly
being tested. Cleanup deletes only the isolated test ACL after readback.
