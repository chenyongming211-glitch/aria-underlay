# H3C Command Adaptation Roadmap - 2026-05-09

This document records the next H3C command-adaptation batches after VLAN,
access/trunk, description, IPv4 advanced ACL, and interface ACL binding support
landed in `main`.

## Current Production Surface

- VLAN create/update through NETCONF running edit-config.
- VLAN name and description.
- Access port PVID.
- Trunk port allowed VLAN list.
- Access/trunk interface description.
- Scoped state readback and verify.
- Numeric IPv4 advanced ACLs in range `3000..=3999`.
- IPv4 advanced ACL rules with permit/deny, ip/tcp/udp/icmp, source/destination
  wildcard endpoints, and TCP/UDP source/destination port `eq`.
- Interface packet-filter binding for an isolated IPv4 advanced ACL.

The following are intentionally out of scope for the current production surface:

- Admin up/down.
- Trunk native VLAN.
- Implicit delete by omitting objects from desired state.
- PBR, QoS traffic-classifier/policy, NQA, and BGP configuration.
- IPv6 ACL, basic ACL, and named ACL.
- Cross-device ACID semantics.

## Batch 1: Low-Risk Completion

### ACL Rule Description Closure

Goal: make the existing `AclRule.description` field real for H3C.

Scope:

- Render ACL rule description in H3C `IPv4AdvanceRules/Rule` XML.
- Parse ACL rule description from H3C running XML.
- Verify desired and observed ACL rule descriptions.
- Add real-device acceptance variables and checklist entries.

Safety:

- Use only an ACL id proven absent by live readback.
- Do not bind the ACL unless the ACL binding case is explicitly being tested.
- Cleanup deletes only the isolated test ACL after readback.

### Explicit Delete Intent Design

Goal: allow production deletes without treating every absent object as a delete.

Required boundary:

- Delete must be explicit in the product/domain request.
- Dry-run must show the exact target and operation before write.
- Real-device probe must reject deletes unless a dedicated delete acknowledgement
  is present.
- Delete execution order must protect references: unbind before deleting an ACL,
  detach policy references before deleting policies or ACLs, and restore ports
  before deleting test VLANs.

First supported delete candidates:

- Delete isolated test ACL by id.
- Delete ACL binding by interface/direction/ACL id.
- Delete isolated test VLAN by id.

Not in Batch 1 implementation:

- Delete PBR, QoS, NQA, or BGP objects.
- Infer delete from missing desired state in merge/upsert mode.

## Batch 2: ACL Family Expansion

Add one ACL family at a time. Do not combine IPv6, basic ACL, and named ACL in a
single implementation batch.

Recommended order:

1. Basic IPv4 ACL, because it is simpler than IPv6 and reuses most rule parsing.
2. Named IPv4 ACL only if real devices expose a stable NETCONF shape.
3. IPv6 ACL after IPv4 variants have parser/renderer/cleanup parity.

Every ACL family must include renderer tests, parser tests, verify tests,
cleanup support where applicable, and a real-device acceptance checklist.

## Batch 3: QoS Traffic Classifier And Policy

Goal: prove ACL references outside packet-filter binding before PBR.

Minimal surface:

- Traffic classifier referencing one test ACL.
- Traffic behavior with a harmless marking or explicitly approved test action.
- Traffic policy binding to an approved idle interface.

Safety:

- Use only isolated test ACLs.
- Require readback proof that no unrelated classifier/policy is changed.
- Cleanup must detach policy first, then delete policy/classifier/behavior, then
  delete the isolated ACL.

## Batch 4: PBR MVP

Minimal surface:

- Policy node.
- `if-match acl` referencing an isolated test ACL.
- Explicitly approved test next-hop.
- Binding only to an approved idle interface or VLAN interface.

Safety:

- PBR can change forwarding behavior; require an explicit real-device
  acknowledgement distinct from generic apply acknowledgement.
- Use dry-run gates to reject update/delete of existing production policies.
- Cleanup must detach PBR before deleting policy nodes or ACLs.

## Batch 5: NQA MVP

Minimal surface:

- Create/read/delete one isolated NQA operation.
- No track, route, or PBR coupling in the first pass.

## Batch 6: BGP

Start with read-only BGP parser support:

- Local AS.
- Neighbor id/address.
- Session state.

Write support should come only after read-only parsing has been validated on
representative devices. BGP writes require separate design review because the
blast radius is larger than VLAN, ACL, or NQA.

## Cross-Device Atomicity

Do not mix this with command adaptation. The next realistic step is clearer
multi-device result reporting, such as an explicit partial-success status and
operator recovery guidance. Full cross-device ACID remains a separate
architecture effort.
