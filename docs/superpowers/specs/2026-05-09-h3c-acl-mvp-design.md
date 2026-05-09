# H3C ACL MVP Design

## Goal

Add a low-risk H3C ACL command surface that can create and verify an isolated
IPv4 advanced ACL through the existing transactional NETCONF path.

The MVP is intentionally narrow: it manages an ACL object only. It does not bind
the ACL to interfaces, VLANs, PBR, QoS, or routing features.

## Scope

Included:

- Rust model/protobuf support for IPv4 advanced ACLs.
- H3C renderer support for numeric advanced IPv4 ACL groups.
- H3C parser support for reading numeric advanced IPv4 ACL groups and rules.
- Scoped read/verify support using `StateScope.acl_ids`.
- Cleanup tooling for deleting an isolated test ACL.
- Real-device acceptance steps that choose a non-existing ACL number before any
  write.

Excluded for this batch:

- ACL binding to interfaces, VLAN interfaces, PBR, QoS, or BGP policies.
- IPv6 ACLs, basic ACLs, named ACLs, object groups, time ranges, ranges, and
  complex port operators.
- Replacing or deleting arbitrary production ACL rules through normal apply.
- Reusing any existing ACL number during real-device acceptance.

## Real H3C Shape

Read-only NETCONF discovery against the S5560/S6800 representatives confirmed
the H3C Comware top-level subtree:

- ACL config lives under `top/ACL`.
- Numeric IPv4 advanced groups use `Groups/Group` with `GroupType=1` and
  `GroupID=<acl_id>`.
- Numeric IPv4 advanced rules use `IPv4AdvanceRules/Rule`.
- `Action=2` means permit; `Action=1` means deny.
- `ProtocolType=256/6/17/1` maps to `ip/tcp/udp/icmp`.
- Source and destination matches use `SrcAny`, `DstAny`, `SrcIPv4`, and
  `DstIPv4` fields.
- TCP/UDP equality ports use `SrcPort` or `DstPort` with `*PortOp=2`,
  `*PortValue1=<port>`, and device readback `*PortValue2=65536`.

## Data Model

The core model will add:

- `AclConfig`: ACL id, optional description, and ordered rules.
- `AclRule`: sequence, action, protocol, optional source/destination IPv4
  wildcard matches, and optional source/destination `eq` port matches.
- `AclEndpoint`: IPv4 address and wildcard.

The model represents only IPv4 advanced ACLs in this MVP. Missing source or
destination means `any`.

## Safety

- Real-device tests must first read existing ACL ids from the switch.
- A candidate ACL id must be absent immediately before writing.
- If the candidate exists, the run stops.
- Cleanup deletes only the test ACL id that was confirmed absent before the run.
- The MVP does not bind ACLs, so an isolated ACL object should not affect live
  forwarding.
- Normal apply must not be used to mutate existing production ACLs until replace
  and ownership semantics are designed.

## Testing

Use TDD:

- Add Rust model/mapper/diff tests for ACL desired state, scope, and observed
  state conversion.
- Add Python renderer tests for H3C ACL XML.
- Add Python parser tests for real H3C ACL XML readback.
- Add backend verification tests for ACL mismatches.
- Add cleanup tests for ACL delete payloads.
- Run focused adapter tests remotely in Docker, then push for GitHub Actions
  Rust/proto verification.

