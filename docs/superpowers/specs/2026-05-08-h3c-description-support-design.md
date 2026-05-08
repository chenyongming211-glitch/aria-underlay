# H3C Description Support Design

## Goal

Add the next low-risk H3C production command surface: VLAN description and
interface description. The change stays inside the existing desired-state
contract and does not add new proto or Rust model fields.

## Scope

Included:

- Render H3C VLAN descriptions as `VLAN/VLANs/VLANID/Description`.
- Render H3C interface descriptions as
  `Ifmgr/Interfaces/Interface/IfIndex/Description`.
- Keep access PVID and trunk allowed VLAN rendering unchanged.
- Include H3C `Ifmgr` in scoped running-state reads so description verification
  has readback data.
- Extend the real-device probe and runbook examples with optional description
  environment variables.
- Extend cleanup tooling so acceptance tests can restore or clear interface
  descriptions after a real-device run.

Excluded for this batch:

- Admin down/up writes.
- Trunk native VLAN/PVID writes.
- Delete semantics through normal apply.
- New product API fields or proto schema changes.
- HA or cross-node coordination.

## Architecture

The Rust core already carries `VlanConfig.description` and
`InterfaceConfig.description`. Those fields map through the existing adapter
proto. The implementation therefore belongs mostly in the Python H3C adapter:
the renderer will stop failing closed for description fields and will emit the
validated Comware XML shape, while the parser/filter path will continue to read
descriptions from H3C `Ifmgr`.

The H3C edit-config document will keep a single NETCONF `<config>` and `<top>`
root. Under `<top>`, it may contain both `<Ifmgr>` for interface descriptions
and `<VLAN>` for VLAN plus port-mode edits. Empty desired state still fails
closed.

## Data Flow

1. Intent/probe builds a desired VLAN or interface with an optional description.
2. Rust maps the existing model field to `VlanConfig.description` or
   `InterfaceConfig.description` in protobuf.
3. `H3cRenderer.render_edit_config` creates:
   - `VLAN/VLANs/VLANID/Description` for VLAN descriptions.
   - `Ifmgr/Interfaces/Interface/Description` keyed by `IfIndex` for interface
     descriptions.
4. NETCONF running verification reads the scoped state. H3C scoped filters
   request both `Ifmgr` and `VLAN`.
5. `H3cStateParser` reads back VLAN and interface descriptions and the existing
   verify path compares them to desired state.

## Safety

- The real-device probe still refuses dry-runs containing delete operations.
- This batch does not introduce admin-state changes, native VLAN changes, or
  delete operations through normal apply.
- Cleanup must be dry-run inspectable and require `--yes` before writing.
- Documentation continues to tell operators to choose approved idle resources
  and verify cleanup readback.

## Testing

Use TDD:

- Renderer tests first fail while H3C rejects description fields.
- State filter test first fails until H3C scoped filters include `Ifmgr`.
- Cleanup script tests first fail until description restore/clear payloads are
  implemented.
- Probe behavior is covered by Rust tests or, where local Rust is unavailable,
  by CI after push.

Remote adapter Docker remains the local verification path because the Windows
workspace does not have the Python/Rust toolchain installed.
