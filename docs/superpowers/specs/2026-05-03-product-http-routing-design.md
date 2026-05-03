# Product HTTP Routing Design

## Goal

Add a product-facing HTTP route contract on top of `ProductOpsApi` so future HTTP handlers can reuse a single RBAC/audit-safe routing layer without calling product operations directly.

## Scope

This package adds method/path/status/body JSON semantics. It does not start a listener, add a web framework, integrate a real identity provider, add UI, or require a real switch.

## Route Contract

The first routes are:

| Method | Path | Request body | Response body |
|---|---|---|---|
| `POST` | `/product/v1/operations/summaries:query` | `ListOperationSummariesRequest` JSON | `ProductApiResponse<ListOperationSummariesResponse>` JSON |
| `POST` | `/product/v1/product-audit:export` | `ExportProductAuditRequest` JSON | `ProductApiResponse<ExportProductAuditResponse>` JSON |

HTTP metadata is carried in headers:

| Header | Required | Purpose |
|---|---:|---|
| `x-aria-request-id` | yes | Operator-visible request ID. |
| `x-aria-trace-id` | no | Cross-service trace ID; defaults to request ID when omitted. |
| `x-aria-operator-id` | yes | Local/mock operator identity. |
| `x-aria-role` | yes | Local/mock RBAC role. |

Header names are matched case-insensitively. Empty required headers are rejected.

## Architecture

Add `src/api/product_http.rs` with a small framework-neutral router:

- `ProductHttpRequest`: method, path, headers, raw JSON body.
- `ProductHttpResponse`: status code, JSON headers, raw JSON body.
- `ProductHttpRouter`: owns a `ProductOpsApi`, converts HTTP metadata into `ProductApiRequest<T>`, dispatches to the correct API method, and maps errors to stable HTTP error responses.

The router is intentionally independent of axum, hyper, or tonic. A future server can adapt its native request type into `ProductHttpRequest` and pass the result through this router. That keeps route behavior and tests stable while leaving the final runtime/server choice open.

## Error Handling

All failures return JSON:

```json
{
  "request_id": "req-123",
  "trace_id": "trace-123",
  "error_code": "invalid_request",
  "message": "missing required product HTTP header x-aria-request-id"
}
```

Status mapping:

| Condition | Status |
|---|---:|
| Invalid JSON, missing required header, invalid route body | `400` |
| Unknown path | `404` |
| Known path with unsupported method | `405` |
| RBAC denial | `403` |
| Product audit write failure or internal serialization/storage failure | `500` |

The `allow` response header is set on `405`.

## Testing

Add Rust contract tests for:

- operation summary HTTP route succeeds with mock viewer session;
- product audit export HTTP route succeeds with mock auditor session and writes audit export evidence;
- operator role is denied audit export with `403`;
- missing request ID returns `400`;
- unknown path returns `404`;
- wrong method on a known route returns `405` with `allow: POST`;
- malformed JSON returns `400`.

Local Rust tooling is unavailable in this workspace, so GitHub Actions remains the Rust compile/test gate.
