# Product Session Identity Boundary Design

## Goal

Replace the product API's implicit mock-header-only identity path with an explicit, fail-closed session identity boundary that can later be backed by a real IdP without changing product operations, RBAC, or audit code.

## Scope

This package adds identity abstraction and local/static verifier coverage. It does not implement OIDC, JWT signature verification, JWKS fetch, SSO login, refresh tokens, browser sessions, or a public HTTP listener.

## Architecture

Add `src/api/product_identity.rs`:

- `ProductAuthenticatedPrincipal`: normalized identity output from a verifier.
- `ProductIdentityVerifier`: trait for validating bearer tokens and returning a principal.
- `StaticProductIdentityVerifier`: deterministic in-memory verifier for tests and local/offline mode.
- `BearerTokenProductSessionExtractor`: `ProductSessionExtractor` implementation that reads `Authorization: Bearer <token>`, validates it through a verifier, and returns the existing `ProductSession`.

The existing `HeaderProductSessionExtractor` remains for local/mock contract tests only. Production-facing HTTP wiring should use `BearerTokenProductSessionExtractor` or a future IdP-backed verifier.

## Semantics

Bearer token extraction:

- Header name is case-insensitive.
- Scheme is `Bearer`, case-insensitive.
- Missing header, malformed scheme, empty token, unknown token, and expired token all fail closed.
- Token verification returns one role for now because the existing RBAC layer currently uses a single request role.

Principal fields:

- `operator_id`
- `role`
- optional `issuer`
- optional `subject`
- optional `session_id`
- optional `expires_at_unix_secs`

Expiry is checked by the verifier using the current Unix time. An expired token fails before any product operation or audit export runs.

## Error Handling

Add `UnderlayError::AuthenticationFailed(String)`.

`ProductHttpRouter` maps authentication failures to:

- HTTP status `401`
- JSON error code `authentication_failed`
- `www-authenticate: Bearer`

Authorization failures remain `403`. This keeps authentication and RBAC denial separate for product operators and future HTTP handlers.

## Testing

Add Rust contract tests for:

- bearer token session can list operation summaries without mock operator/role headers;
- missing bearer token fails with `AuthenticationFailed`;
- unknown token fails with `AuthenticationFailed`;
- expired token fails with `AuthenticationFailed`;
- product HTTP route maps authentication failure to `401` and `www-authenticate: Bearer`;
- existing mock header extractor behavior remains available.

Local Rust tooling is unavailable in this workspace, so GitHub Actions remains the Rust compile/test gate.
