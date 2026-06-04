# Domain ID Serial Apply Design

## Context

P1 product request defenses already include request-level `idempotency_key` reuse and file-backed idempotency retention. The next missing control-plane guard is orchestration serialization: product or gateway retries and parallel requests can target the same underlay domain while touching different endpoints. Endpoint locks protect a single device, but they do not express the product-level rule that one underlay management domain should apply one intent at a time.

## Decision

The first implementation uses `UnderlayDomainIntent.domain_id` as the orchestration key. `AriaUnderlayService::apply_domain_intent` acquires a local in-process domain lock before planning, idempotency lookup, idempotency persistence, and actual endpoint apply. Requests for the same `domain_id` run serially. Requests for different `domain_id` values may run concurrently.

This deliberately does not claim global ACID across endpoints. It only narrows inconsistent windows and prevents control-plane interleaving for the same domain. Later work can extend the key to explicit `region_id`, switch-pair, or custom orchestration scope without changing endpoint transaction semantics.

## Scope

In scope:

- Add a reusable domain lock table with deterministic key normalization based on `domain_id`.
- Wire the lock into `apply_domain_intent`.
- Keep `dry_run_domain` unlocked because it does not write journal, shadow, adapter state, or idempotency records.
- Keep endpoint locks unchanged; domain lock is acquired outside endpoint locks.
- Add tests showing same-domain apply calls serialize and different-domain apply calls can proceed independently.

Out of scope:

- Cross-process or HA distributed locking. Active/passive HA remains the production guard for multi-core deployment.
- New API fields such as `region_id` or `orchestration_scope`.
- Cross-device rollback or global ACID semantics.
- Product HTTP apply route wiring, because this repository still has no product domain-apply HTTP route.

## Data Flow

1. `apply_domain_intent` clones `request.intent.domain_id`.
2. Service acquires `DomainApplyLockTable` for that domain.
3. Service normalizes idempotency key and computes the apply fingerprint.
4. If an idempotency record exists, return the reused response while holding the domain lock.
5. Otherwise plan and apply desired endpoint states through the existing `ApplyCoordinator`.
6. Persist the idempotency record if requested.
7. Drop the domain lock when the request completes.

## Error Handling

The lock is an async local mutex and does not introduce a timeout in the first implementation. Endpoint-level lock timeout behavior remains unchanged and still returns `ENDPOINT_LOCK_TIMEOUT` when device contention exceeds policy. If lock table internals fail due to poisoned synchronous bookkeeping, return `Internal`.

## Testing

Tests should prove behavior, not implementation details:

- Same `domain_id`: two concurrent apply requests serialize; the second cannot reach adapter prepare while the first is held.
- Different `domain_id`: a request for domain B can reach adapter prepare while domain A is held.
- Idempotency reuse still works while the domain lock is enabled.

CI verification is GitHub Actions because the local Windows environment does not have `cargo`.
