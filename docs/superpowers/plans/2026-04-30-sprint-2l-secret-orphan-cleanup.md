# Sprint 2L: Secret Orphan Cleanup

## Goal

Close the device bootstrap gap where site initialization creates a device-scoped secret and then leaves it orphaned when inventory registration fails.

## Design

- Treat secret provisioning as a compensatable operation.
- Distinguish credentials created by the current request from `ExistingSecretRef` values supplied by the caller.
- Clean up only secrets owned by the current initialization attempt.
- If cleanup fails, fail visibly by returning the retained `secret_ref` plus both the registration and cleanup errors.

## Implementation Tasks

1. Add `SecretProvisioningResult` with `secret_ref` and `cleanup_on_registration_failure`.
2. Change `SecretStore::create_for_device` to return `SecretProvisioningResult`.
3. Add `SecretStore::delete(secret_ref)` for compensating cleanup.
4. Update `InMemorySecretStore` to mark password/private-key credentials as cleanup-owned and existing refs as caller-owned.
5. Update `UnderlaySiteInitializationService` to delete cleanup-owned secrets when registration fails.
6. Add regression tests for successful cleanup and cleanup failure surfacing.
7. Mark the bug inventory item fixed and record the progress.

## Non-Goals

- Do not delete `ExistingSecretRef` values on registration failure.
- Do not roll back successfully registered devices when another device in the same site fails.
- Do not make onboarding failure delete inventory or secrets; current product semantics remain `PartiallyRegistered`.

## Verification

- Local Rust verification is unavailable in the current environment because `cargo` is not installed.
- GitHub Actions must validate `cargo check`, `cargo test`, and formatting after push.
