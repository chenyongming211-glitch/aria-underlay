# Adapter Client Pool Design

## Context

Rust core currently creates a new `AdapterClient::connect(endpoint)` for onboarding, dry-run state reads, apply, recovery, drift audit, and force unlock. That works in tests, but it creates avoidable gRPC channel churn and gives production no central place to manage endpoint channel lifecycle.

## Decision

Add an `AdapterClientPool` in `src/adapter_client/` that caches tonic `Channel` handles by adapter endpoint. Callers request a fresh `AdapterClient` facade from the pool for each operation; the facade owns an `UnderlayAdapterClient<Channel>` built from the cached channel clone.

This keeps the pool boundary at the adapter client layer and avoids sharing one mutable RPC client across tasks. tonic `Channel` is already cheap to clone and supports HTTP/2 multiplexing and reconnect behavior, so caching channels is the right first production step.

## Behavior

- Same endpoint reuses one cached channel handle.
- Different endpoints receive separate cached channels.
- Invalid endpoint URIs fail before a client is returned.
- Transport failures during RPC are still surfaced by the existing `AdapterClient` methods.
- The pool exposes a small `invalidate(endpoint)` hook for future failure policies and operational controls.

## Integration

- `AriaUnderlayService` owns one `AdapterClientPool`.
- `DeviceOnboardingService` owns or receives the same pool where it is called from `AriaUnderlayService` and site initialization.
- Existing `AdapterClient::connect(endpoint)` remains for examples and direct probes.
- Default constructors create a default pool; tests can inject one if needed.

## Non-Goals

- No idle eviction in this sprint.
- No max-per-endpoint object pool; tonic channel multiplexing makes that unnecessary for the current architecture.
- No health-check worker in this sprint.
- No real-switch validation dependency.
