# Apply Verify Report Design

## Context

Underlay already performs scoped post-commit verify inside the transaction
pipeline. That verify result currently affects transaction outcome, but product
callers only see the final endpoint status and generic warnings. Operators need
a first-class report that explains which endpoints were verified, what touched
scope was checked, and which endpoints still need attention.

## Goals

- Add a product-visible `verify_report` to `DeviceApplyResult`.
- Add an aggregate `verify_report` to `ApplyIntentResponse`.
- Preserve existing transaction semantics: verify failure still drives rollback
  or `InDoubt` through the existing apply coordinator path.
- Include scoped verify details: source, status, touched VLAN/interface/ACL
  counts, warnings, and error fields.
- Persist verify reports through the existing domain apply record store so
  compensation and later product routes can show verify state without reading
  raw journal files.

## Non-Goals

- No background verify worker.
- No automatic reconciliation.
- No change to adapter protobuf schema.
- No new Product HTTP route in this phase.
- No weakening of existing verify failure handling.

## Report Model

Each endpoint result gets:

- `status`: `passed`, `failed`, `skipped`, or `in_doubt`
- `source`: `adapter_scoped_verify`
- `scope`: summary of the touched scope checked by verify
- `warnings`
- `error_code`
- `error_message`

The apply response aggregate gets:

- `status`: `passed`, `failed`, `partial`, `in_doubt`, or `skipped`
- endpoint id lists for passed, failed, skipped, and in-doubt endpoints
- `attention_required`
- `warning_count`

## Semantics

- Changed endpoints with successful adapter verify receive `passed`.
- Verify adapter errors or non-success statuses receive `failed` or `in_doubt`
  and continue through existing rollback/failure handling.
- No-op endpoints receive `skipped`.
- Preflight failures before verify receive `skipped` unless they become
  `InDoubt`, in which case the report is `in_doubt`.
- Degraded transaction strategies can still pass verify but preserve their
  existing warnings.

## Testing

- Unit tests cover aggregate report status.
- Integration tests assert successful apply includes endpoint and aggregate
  verify reports with scoped counts.
- Verify failure tests assert report status exposes the verify failure while
  preserving rollback/InDoubt behavior.
- Serialization tests ensure the new optional fields are backwards-compatible
  for older JSON fixtures.
