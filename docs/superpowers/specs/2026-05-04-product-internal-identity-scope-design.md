# Product Internal Identity Scope Design — 2026-05-04

## Goal

Correct the product API identity direction for an internal-only deployment.
The product API must not implement SSO, OIDC, JWT, JWKS, refresh tokens, browser
sessions, or external identity-provider discovery in this repository.

## Design

Keep the existing `ProductIdentityVerifier` abstraction because it separates
authentication from RBAC and product audit. The packaged verifier is
`StaticProductIdentityVerifier`, wired through
`BearerTokenProductSessionExtractor` and `ProductApiServerConfig.static_tokens`.

`ProductApiServerConfig` should be strict about its JSON shape. Unknown fields
fail at parse time, so historical identity fields such as `jwt_jwks` and
`jwt_jwks_file` cannot be silently ignored.

Production deployments still run behind an internal ingress or host policy for:

- TLS termination
- client authentication, if the site requires it
- rate limiting
- proxy/header policy
- operator-network restrictions

Those controls are deployment boundaries, not product API identity features in
this repo.

## Operational Semantics

Internal bearer tokens map to normalized principals:

- `operator_id`
- role
- optional issuer
- optional subject
- optional session ID
- optional expiry

Token lifecycle, rotation, revocation, and audit-friendly replacement of
`static_tokens` are separate internal operations work and should be designed
before treating this as a long-term production identity store.

## Out Of Scope

- SSO
- OIDC discovery
- JWT signature verification
- JWKS storage or refresh
- browser sessions
- refresh tokens
- external enterprise IM or paging delivery
