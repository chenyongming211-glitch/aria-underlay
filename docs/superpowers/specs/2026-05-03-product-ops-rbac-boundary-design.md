# Product Ops RBAC Boundary Design

## Goal

Add a product-facing operations boundary that applies RBAC before operator-facing reads and records product audit before exporting audit history.

## Scope

Included:

- A Rust `ProductOpsManager` for future product API handlers.
- Operator context validation for product operations.
- RBAC-gated operation summary listing.
- RBAC-gated product audit export.
- Audit-before-export semantics for product audit history.
- Tests for allowed, denied, and audit-write-failure paths.

Excluded:

- HTTP routing.
- Identity provider integration.
- Token/session parsing.
- UI work.
- Real switch operations.
- Online daemon reload.

## Design

`ProductOpsManager` will live in `src/api/product_ops.rs`. It owns:

- `Arc<dyn AuthorizationPolicy>`
- `Arc<dyn OperationSummaryStore>`
- `Arc<dyn ProductAuditStore>`

Product API handlers can construct this manager with the same stores and authorization policy already used by service/admin operations.

The boundary exposes:

- `list_operation_summaries(context, request)`
- `export_product_audit(context, request)`

`ProductOperatorContext` contains `request_id`, optional `trace_id`, and `operator`. It is required for product-facing operations even when the underlying local store query does not need it.

## Authorization

Operation summary listing authorizes `AdminAction::ListOperationSummaries`. The existing RBAC matrix allows any assigned role to list summaries, while unassigned operators fail closed.

Product audit export authorizes `AdminAction::ExportAuditHistory`. The existing RBAC matrix allows `Admin` and `Auditor`, and denies `Viewer`, `Operator`, and `BreakGlassOperator`.

## Audit Semantics

Product audit export is itself sensitive, so the manager appends a `product_audit.export_requested` record before returning audit records. If appending that audit record fails, the export fails and no history is returned.

Operation summary listing is read-only and not recorded as product audit in this package. It is still RBAC-gated so product handlers cannot accidentally expose summaries without operator identity.

## Filtering

The manager supports server-side filters needed by the first product API layer:

- operation summaries: reuse `ListOperationSummariesRequest`
- product audit export: `action`, `result`, `operator_id`, and `limit`

Limit behavior keeps the newest matching records when a limit is provided, matching the existing operation summary query behavior.

## Testing

Tests cover:

- Viewer with an assigned role can list operation summaries.
- Unassigned operator cannot list operation summaries.
- Auditor can export product audit and the export action is itself recorded.
- Operator cannot export product audit.
- Product audit write failure blocks audit export.

Local Rust tests may be unavailable on this workstation because `cargo` is not installed. GitHub Actions remains the Rust compile and test gate.
