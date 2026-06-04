# Configurable Apply Scope Lock Design

## Context

Underlay currently serializes `apply_domain_intent` by `domain_id`. That is the
right safe default, but product deployments need more explicit orchestration
choices:

- `domain`: serialize all applies for the same underlay domain.
- `region`: serialize applies across multiple domains that share an operator
  supplied region id.
- `switch_pair`: serialize only applies that touch the same managed endpoint or
  switch-pair scope, allowing unrelated pairs in one domain to proceed.

This is a product request defense. It prevents control-plane interleaving and
narrows inconsistent windows; it does not create cross-device ACID rollback.

## Decision

Add `ApplyOptions.lock_scope` with default `Domain`. Add optional
`ApplyOptions.region_id`, required only when `lock_scope` is `Region`.

`AriaUnderlayService::apply_domain_intent` resolves the requested lock scope to
one or more deterministic lock keys before idempotency lookup, planning, apply,
and persistence. The existing local async lock table is extended from one
domain key to ordered multi-key acquisition so switch-pair scopes can safely
lock all touched endpoint keys without deadlock.

## Scope Semantics

`Domain`:

- Key: `domain:<domain_id>`
- Default for old clients and omitted `lock_scope`.
- Preserves current behavior.

`Region`:

- Key: `region:<region_id>`
- Fails closed if `region_id` is missing or blank.
- Lets product serialize multiple domains in a physical region.

`SwitchPair`:

- Keys: `switch_pair:<domain_id>:<endpoint_id>` for each management endpoint
  touched by the intent.
- Endpoints are deduplicated and acquired in sorted order.
- Fails closed if the intent has no endpoints.
- For MLAG dual-management intents this locks both ToR endpoints; for
  single-stack intents this locks the one management endpoint.

## Non-Goals

- No distributed lock or cross-process lock in this phase.
- No change to endpoint transaction semantics.
- No new product HTTP route.
- No automatic pair discovery outside the submitted intent.
- No global ACID semantics for multi-device apply.

## Data Flow

1. `apply_domain_intent` clones the request and resolves lock keys from
   `request.options` and `request.intent`.
2. The service acquires the ordered apply scope guard.
3. Idempotency lookup and reuse run under the guard.
4. Planning, endpoint apply, verify reporting, apply record persistence, and
   idempotency persistence run under the guard.
5. The guard drops when the request returns.

## Error Handling

- Empty `domain_id`, `region_id`, or endpoint ids return `InvalidIntent`.
- Unknown JSON fields in `ApplyOptions` remain rejected through
  `deny_unknown_fields`.
- Missing `lock_scope` and missing `region_id` for non-region scopes remain
  backward compatible.
- Multi-key acquisition sorts keys before locking so concurrent switch-pair
  requests cannot deadlock by submitting endpoints in different order.

## Testing

- Request tests cover default lock scope, region parsing, and unknown-field
  rejection.
- Lock table tests cover ordered multi-key acquisition and invalid keys.
- Service tests cover:
  - default domain scope still serializes same-domain applies;
  - region scope serializes different domains with the same region id;
  - switch-pair scope allows different endpoint scopes in the same domain to
    progress independently;
  - switch-pair scope serializes overlapping endpoint scopes.

CI remains the compile and test gate because this Windows workspace does not
have local Rust tooling installed.
