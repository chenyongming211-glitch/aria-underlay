# PartialSuccess Failed Endpoint Retry Design

## Context

Underlay already reports mixed multi-endpoint outcomes as `PartialSuccess`,
and each changed endpoint has its own transaction journal entry. That is
correct for device safety, but it is not yet productized enough for operators:
after one endpoint succeeds and another fails, the caller must manually infer
which endpoints can be retried, which require recovery, and how to avoid
touching already successful endpoints.

This design adds a control-plane compensation layer for domain applies. It
does not introduce cross-device ACID rollback. It makes the existing
per-endpoint semantics easier to inspect and safely retry.

## Goals

- Persist enough domain apply context to reconstruct a failed-endpoint retry.
- Let callers inspect an apply by `request_id` and see endpoint retry
  classification.
- Retry only terminal failed endpoints by default.
- Never retry `InDoubt` endpoints through the compensation path; they must use
  recovery or manual force-resolve first.
- Keep successful endpoints out of the retry intent so they are not re-applied.

## Non-Goals

- No cross-device rollback or global ACID transaction.
- No Product HTTP route in this phase. The current product HTTP router is
  synchronous and does not expose domain apply routes; this phase lands the
  service/API primitive first.
- No retry of legacy `SwitchPairIntent`; this feature is for
  `ApplyDomainIntentRequest`.
- No automatic background retry loop. Operators or product callers choose when
  to retry.

## Architecture

Add a `DomainApplyRecordStore` alongside the existing idempotency and journal
stores. Each completed non-reused `apply_domain_intent` stores:

- original `ApplyDomainIntentRequest`
- final `ApplyIntentResponse`
- `domain_id`
- `created_at_unix_secs` and `updated_at_unix_secs`

The store has in-memory and JSON file implementations. `AriaUnderlayService`
uses an in-memory store by default and exposes a builder for file-backed
production use.

Add a compensation module with two service methods:

- `get_domain_apply_compensation_plan(request_id)`
- `retry_failed_domain_endpoints(request)`

The plan classifies endpoint results into:

- `retryable_failed`: `Failed` or `RolledBack`
- `requires_recovery`: `InDoubt`
- `completed`: `Success`, `SuccessWithWarning`, or `NoOpSuccess`

The retry method loads the original record, selects retryable failed endpoints
unless the caller provides an explicit endpoint list, builds a sub-intent that
contains only the selected management endpoints and their members, and then
calls `apply_domain_intent` with a new request id.

## Data Flow

1. Product or service caller submits `ApplyDomainIntentRequest`.
2. `apply_domain_intent` executes the existing plan/apply path.
3. The final response is stored in `DomainApplyRecordStore`.
4. Caller asks for a compensation plan by original `request_id`.
5. Caller invokes retry with a new `request_id` and the original request id.
6. Underlay filters the original intent to the retryable endpoints and applies
   only that sub-intent.

## Error Handling

- Missing original request id returns `InvalidIntent`.
- No retryable failed endpoints returns `InvalidIntent`.
- Explicit endpoint ids that were not failed terminal endpoints return
  `InvalidIntent`.
- Any original endpoint in `InDoubt` is reported in the plan and excluded from
  automatic retry.
- Apply record persistence failures are added to the apply response warnings
  instead of changing device transaction outcome.

## Testing

- Unit test endpoint classification.
- Unit test domain intent filtering keeps selected endpoints and their member
  scoped interface/ACL bindings while excluding completed endpoints.
- Integration test a two-endpoint `PartialSuccess`, verify the plan marks only
  the failed endpoint as retryable, then retry and assert the successful
  endpoint adapter is not called again.
- File-backed store round-trip test so retry context survives service
  recreation.

## Future Work

- Product HTTP route once async domain apply routing exists.
- CLI wrapper for compensation plan and retry.
- Optional policy to allow retrying explicitly recovered `InDoubt` endpoints
  after a fresh recovery report proves they are terminal.
