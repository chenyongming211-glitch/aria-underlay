# Product JWT/JWKS Identity Implementation Plan — 2026-05-04

## Scope

Add an offline JWT/JWKS product identity verifier and wire it into the local
product API binary.

## Steps

1. Add focused tests.
   - valid signed token maps to `ProductAuthenticatedPrincipal`
   - wrong audience fails closed
   - unknown `kid` fails closed
   - unmapped role fails closed
   - empty JWKS config is rejected

2. Implement verifier types in `product_identity`.
   - `ProductJwtAlgorithm`
   - `ProductJwtJwksVerifierConfig`
   - `JwtJwksProductIdentityVerifier`

3. Wire `aria-underlay-product-api`.
   - support `jwt_jwks`
   - reject configs containing both `static_tokens` and `jwt_jwks`

4. Add checked-in local JWKS config sample.

5. Update docs and bug inventory.

6. Verify locally where possible, then push and use GitHub Actions for Rust.
