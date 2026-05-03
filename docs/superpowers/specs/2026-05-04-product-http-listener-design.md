# Product HTTP Listener Design — 2026-05-04

## Goal

Bind the existing framework-neutral `ProductHttpRouter` to a real local HTTP
listener without introducing a real switch dependency or pretending that the
production identity provider is complete.

## Design Choice

Use a narrow Tokio TCP HTTP/1.1 adapter in `src/api/product_http_server.rs`.
The adapter is intentionally small and replaceable:

- It parses one HTTP/1.1 request per connection.
- It requires `Content-Length` semantics and rejects unsupported transfer
  encodings.
- It enforces a configurable request body limit before dispatching to the
  router.
- It always closes the connection after the response.
- It delegates all product route behavior to `ProductHttpRouter`.

This is not a general web framework. It is a stable product API runtime seam
that keeps product routing, RBAC, identity extraction, and audit behavior in the
existing tested layers. If the deployment later selects axum, hyper, or an
internal gateway, that server can adapt native request objects into
`ProductHttpRequest` and keep route behavior unchanged.

## Runtime Entry

Add `aria-underlay-product-api` as a standalone binary. The binary reads a JSON
config path from the first CLI argument or `ARIA_UNDERLAY_PRODUCT_API_CONFIG`.
The local config includes:

- bind address
- max body bytes
- operation summary JSONL path
- product audit JSONL path
- static bearer-token principals for offline/local operation

The checked-in sample binds to `127.0.0.1:8088`. Operators should keep this
listener behind local access controls until the real IdP verifier and production
deployment model are selected.

## Error Handling

Malformed HTTP, invalid content length, unsupported transfer encoding,
incomplete body, and oversized body fail before router dispatch. Errors are
returned as the same JSON shape used by `ProductHttpRouter`:

- `400` for malformed HTTP.
- `413` for payloads above the configured limit.
- router-owned status codes for product route errors.

The server includes `content-length` and `connection: close` on all responses.

## Tests

Add focused tests for:

- loopback TCP listener serving a real product summary query through
  `ProductHttpRouter`
- body limit enforcement before product router dispatch

Local Rust tooling is currently unavailable in this workspace, so GitHub
Actions remains the Rust compile/test gate.

## Out Of Scope

- TLS termination.
- real IdP/JWT/JWKS verification.
- product UI.
- product audit database backend.
- external alert delivery.
- real switch integration.
