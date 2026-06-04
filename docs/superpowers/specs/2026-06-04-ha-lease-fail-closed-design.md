# HA Lease Fail-Closed Design

## Goal

Production write entrypoints must not run as a plain `AriaUnderlayService` when HA protection is required. They must acquire an active-passive lease before accepting writes, and startup must fail closed when the required lease configuration is missing or already held by another Core.

## Context

Aria Underlay already has `ActiveLeaseGuard` and `ActivePassiveAriaUnderlayService`. The wrapper checks `ensure_current()` before service operations and startup recovery runs after lease acquisition. The remaining gap is productized startup selection: callers can still construct the plain service and expose write methods without proving they intentionally disabled HA or acquired a lease.

## Design

Add a small HA startup policy around the existing service:

- `HaLeaseMode::StandaloneAllowed` keeps development and single-node tests unchanged.
- `HaLeaseMode::RequireActiveLease` requires `ActiveLeaseConfig`.
- `AriaUnderlayService::activate_with_ha_policy(policy)` returns a service enum that implements `UnderlayService`.

When `RequireActiveLease` has no lease config, startup returns `HA_LEASE_REQUIRED`. When another Core holds the lease, startup returns the existing `HA_LEASE_HELD`. When the lease is acquired, the returned active service runs startup recovery before exposing writes.

The policy is a startup/assembly guard, not a distributed lock replacement. `EndpointLockTable` and domain/scope locks remain process-local; cross-node safety depends on fencing plus the active lease.

## Out Of Scope

- Active-active coordination.
- Per-request lease acquisition.
- Changing existing test constructors or examples that intentionally use plain local services.
- Product HTTP TLS/mTLS.

## Testing

Add HA tests that verify:

- Required HA mode without `ActiveLeaseConfig` fails with `HA_LEASE_REQUIRED`.
- Required HA mode acquires the lease and exposes an active wrapper.
- Required HA mode fails with `HA_LEASE_HELD` while another owner holds the lease.
- Standalone-allowed mode returns a plain service for local/test use.

CI remains the Rust compile/test gate because the local Windows workspace does not have `cargo` installed.
