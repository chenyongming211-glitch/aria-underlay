# Product Session Identity Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fail-closed product session identity boundary with bearer-token verification abstractions.

**Architecture:** Create `src/api/product_identity.rs` with a verifier trait, static verifier, authenticated principal, and bearer-token session extractor. Keep `ProductOpsApi` and RBAC unchanged by adapting verified principals into the existing `ProductSession`. Extend product HTTP error mapping so authentication failures become `401` while RBAC denials remain `403`.

**Tech Stack:** Rust, serde, existing `ProductSessionExtractor`, existing `RbacRole`, existing `ProductHttpRouter`.

---

### Task 1: Identity Contract Tests

**Files:**
- Create: `tests/product_identity_tests.rs`

- [ ] **Step 1: Write tests first**

Add tests for bearer success, missing bearer token, unknown token, expired token, and HTTP `401` mapping.

- [ ] **Step 2: Run focused test**

Run: `cargo test --test product_identity_tests`

Expected locally: unavailable because `cargo` is not installed. Expected in GitHub Actions before implementation: compile failure because `product_identity` does not exist.

### Task 2: Identity Boundary Implementation

**Files:**
- Create: `src/api/product_identity.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/error.rs`

- [ ] **Step 1: Add `AuthenticationFailed` error**

Add `UnderlayError::AuthenticationFailed(String)`.

- [ ] **Step 2: Add verifier and principal types**

Implement `ProductAuthenticatedPrincipal`, `ProductIdentityVerifier`, and `StaticProductIdentityVerifier`.

- [ ] **Step 3: Add bearer token extractor**

Implement `BearerTokenProductSessionExtractor` as a `ProductSessionExtractor`.

### Task 3: HTTP Error Mapping

**Files:**
- Modify: `src/api/product_http.rs`
- Modify: `src/api/apply.rs`

- [ ] **Step 1: Map auth errors**

Map `AuthenticationFailed` to HTTP `401`, JSON error code `authentication_failed`, and `www-authenticate: Bearer`.

- [ ] **Step 2: Update exhaustive internal error mapping**

Map authentication failures to `AUTHENTICATION_FAILED` in journal/error fields for completeness.

### Task 4: Docs

**Files:**
- Modify: `docs/runbooks/operator-operations.md`
- Modify: `docs/progress-2026-04-26.md`
- Modify: `docs/bug-inventory-current-2026-05-01.md`

- [ ] **Step 1: Document identity boundary**

Record the distinction between local mock headers, bearer-token verifier abstraction, RBAC, and future real IdP.

- [ ] **Step 2: Update current open debt**

Move identity/session validation from fully open to abstraction complete; leave real OIDC/JWT/JWKS and listener exposure open.

### Task 5: Verification and CI

**Files:**
- All package files above.

- [ ] **Step 1: Run local checks**

Run:

```bash
git diff --check
python3 -m pytest adapter-python/tests -q
cargo test --test product_identity_tests
```

Expected: diff and Python pass locally; cargo is unavailable locally.

- [ ] **Step 2: Commit and push**

Commit message: `feat: add product session identity boundary`

- [ ] **Step 3: Wait for GitHub Actions**

Wait for the CI run for the pushed commit. If it fails, inspect logs, fix, and repeat.
