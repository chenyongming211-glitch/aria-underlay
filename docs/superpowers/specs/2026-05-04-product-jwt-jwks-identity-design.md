# Product JWT/JWKS Identity Design — 2026-05-04

## Goal

Move the product API identity boundary beyond static bearer tokens by adding a
real JWT signature and claims verifier behind `ProductIdentityVerifier`.

This package still avoids external runtime dependencies:

- no live IdP call
- no OIDC discovery
- no online JWKS refresh
- no real switch dependency

## Selected Approach

Add an offline JWKS verifier using the `jsonwebtoken` crate.

The verifier accepts a configured JWKS document and validates:

- JWT header `kid` must exist and match a configured JWKS key.
- JWT algorithm must be in the configured allow-list.
- signature verification must pass.
- `exp`, `iss`, `aud`, and `sub` are required.
- `iss` and `aud` must match config.
- `nbf` is enforced when present.
- operator ID is read from a configured claim or falls back to `sub`.
- product role is read from a configured claim and mapped into `RbacRole`.

Role mapping is fail-closed. Unknown roles are rejected. If an array claim maps
to multiple underlay roles, the token is rejected instead of guessing which role
to use.

## Config Boundary

`aria-underlay-product-api` can now use either:

- `static_tokens`: deterministic local/offline tokens
- `jwt_jwks`: signed JWT verification against a configured JWKS

Supplying both is invalid and prevents startup.

## Out Of Scope

- HTTP fetching of JWKS.
- JWKS refresh/cache expiry.
- OIDC discovery.
- browser login/session flows.
- TLS/ingress selection.

Those are separate lifecycle and deployment problems and should be designed
after the product API packaging model is selected.
