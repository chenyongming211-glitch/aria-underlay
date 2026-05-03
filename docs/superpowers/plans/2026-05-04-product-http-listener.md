# Product HTTP Listener Implementation Plan — 2026-05-04

## Scope

Implement the first local product HTTP listener package.

## Steps

1. Add `tests/product_http_server_tests.rs`.
   - Verify loopback TCP request reaches `ProductHttpRouter`.
   - Verify oversized body returns `413` without router dispatch.

2. Add `src/api/product_http_server.rs`.
   - Define `ProductHttpListenerConfig`.
   - Define `ProductHttpServer`.
   - Parse HTTP/1.1 request line, headers, content length, and body.
   - Reject malformed requests and unsupported transfer encodings.
   - Encode router responses as HTTP/1.1 with `content-length` and
     `connection: close`.

3. Add `aria-underlay-product-api`.
   - Read JSON config from CLI argument or `ARIA_UNDERLAY_PRODUCT_API_CONFIG`.
   - Wire `BearerTokenProductSessionExtractor`,
     `StaticProductIdentityVerifier`, `JsonFileOperationSummaryStore`, and
     `JsonFileProductAuditStore`.
   - Run until Ctrl-C.

4. Add checked-in local config sample.

5. Update docs.

6. Verify.
   - Run local checks available in this workspace.
   - Push and wait for GitHub Actions because local Rust tooling is unavailable.
