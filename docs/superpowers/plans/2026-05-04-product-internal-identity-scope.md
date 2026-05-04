# Product Internal Identity Scope Implementation Plan — 2026-05-04

**Goal:** Remove the JWT/JWKS product identity package and make the product API
strictly internal-token based.

### Task 1: Config Contract

Files:
- `src/api/product_server_config.rs`
- `tests/product_api_server_config_tests.rs`

- [x] Require `static_tokens` for product API startup.
- [x] Allow `production_ingress` with internal bearer tokens.
- [x] Add `deny_unknown_fields` so historical `jwt_jwks` and `jwt_jwks_file`
      configs fail closed.
- [x] Add a regression test for rejected JWT/JWKS fields.

### Task 2: Identity Code Cleanup

Files:
- `src/api/product_identity.rs`
- `Cargo.toml`
- `tests/product_jwt_identity_tests.rs`

- [x] Remove JWT/JWKS verifier types.
- [x] Remove the `jsonwebtoken` dependency.
- [x] Delete JWT/JWKS-specific tests.
- [x] Keep bearer-token extraction and static verifier tests.

### Task 3: Samples And Docs

Files:
- `docs/examples/product-api.production.json`
- `docs/examples/tmpfiles.d/aria-underlay.conf`
- `docs/runbooks/operator-operations.md`
- `docs/progress-2026-04-26.md`
- `docs/bug-inventory-current-2026-05-01.md`
- `docs/superpowers/*`

- [x] Replace production sample with internal `static_tokens`.
- [x] Delete JWKS sample files and directory ownership.
- [x] Mark SSO/OIDC/JWT/JWKS as out of scope.
- [x] Record internal token lifecycle as remaining work.

### Task 4: Verification

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests -q
python3 -m json.tool docs/examples/product-api.local.json
python3 -m json.tool docs/examples/product-api.production.json
cargo test --all-targets
```

Expected locally: Python and JSON checks pass; Rust test command is unavailable
when `cargo` is not installed, so GitHub Actions is the Rust gate.
