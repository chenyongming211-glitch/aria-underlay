# Product API Routing Skeleton Design

## Goal

Add a handler-facing product operations API skeleton so future HTTP handlers can call a single RBAC/audit-safe boundary instead of wiring product operations directly.

## Scope

Included:

- A generic product API request envelope.
- A generic product API response envelope.
- A mock header-based product session extractor.
- A `ProductOpsApi` facade for operation summary listing and product audit export.
- Contract tests for success, missing identity, role denial, and audit-write-failure paths.

Excluded:

- Real HTTP server or router.
- Identity provider integration.
- Token/session cryptographic validation.
- Product UI.
- Real switch access.
- Online daemon reload.

## Design

`ProductOpsApi` lives in `src/api/product_api.rs`. It is a thin handler-facing facade over `ProductOpsManager`. It accepts `ProductApiRequest<T>` values with:

- `request_id`
- optional `trace_id`
- string headers
- typed body

The API extracts a `ProductSession` from request metadata through a `ProductSessionExtractor` trait. The first implementation is `HeaderProductSessionExtractor`, which reads:

- `x-aria-operator-id`
- `x-aria-role`

This is explicitly a mock/local extractor for contract tests and local product integration. It is not a trusted production identity model.

For each request, `ProductOpsApi` builds a request-scoped `StaticAuthorizationPolicy` from the extracted session and calls `ProductOpsManager`. This keeps the business operation path behind the same RBAC/audit rules introduced by the product ops boundary while keeping identity extraction replaceable later.

## Behavior

`list_operation_summaries`:

- requires a valid session,
- authorizes `ListOperationSummaries`,
- returns `ProductApiResponse<ListOperationSummariesResponse>`,
- does not write product audit in this package.

`export_product_audit`:

- requires a valid session,
- authorizes `ExportAuditHistory`,
- writes `product_audit.export_requested` before returning records,
- fails closed if audit append fails.

## Testing

Tests cover:

- summary list succeeds with a mock viewer session,
- missing operator header is rejected,
- audit export succeeds with a mock auditor session and records the export,
- audit export is denied for a mock operator session,
- audit export fails closed when product audit append fails.

Local Rust tests may be unavailable on this workstation because `cargo` is not installed. GitHub Actions remains the Rust compile and test gate.
