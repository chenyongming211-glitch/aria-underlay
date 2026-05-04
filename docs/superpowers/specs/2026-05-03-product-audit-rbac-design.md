# Product Audit Backend and RBAC Design

## Goal

Move from local JSONL operations files to a product-grade audit and authorization model without changing the transaction, GC, drift, or recovery code paths that emit operation events.

## Current Local Mode

Current local mode is implemented through these traits and stores:

- `OperationSummaryStore`
- `OperationAlertSink`
- `OperationAlertCheckpointStore`
- `RecordingEventSink`
- `JsonFileOperationSummaryStore`
- `JsonFileOperationAlertSink`
- `JsonFileOperationAlertCheckpointStore`

This is suitable for development, offline validation, and small local runs. It is not enough for multi-operator production because it has no identity boundary, no query API beyond local files, and no central retention policy.

## Backend Boundary

Production should add product-backed implementations behind existing operation interfaces:

| Interface | Product implementation |
| --- | --- |
| `OperationSummaryStore` | Append immutable event-derived summaries to the audit database. |
| `OperationAlertSink` | Store alerts in the internal product alert store for query and lifecycle handling. |
| `OperationAlertCheckpointStore` | Store delivery cursor/dedupe keys in durable product storage. |
| `EventSink` | Fan out to product audit plus metrics, with explicit failure behavior. |

The transaction path should continue to emit `UnderlayEvent` values. It should not depend on a database schema directly.

## Required Audit Fields

Every product audit record must preserve:

- `request_id`
- `trace_id`
- `action`
- `result`
- `tx_id`
- `device_id`
- `operator_id` when the action is operator-initiated
- `role`
- `reason` for break-glass operations
- `attention_required`
- `error_code`
- `error_message`
- structured `fields`
- append timestamp assigned by the audit backend

Audit records are append-only. Corrections are new records, not mutations.

## Roles

| Role | Purpose |
| --- | --- |
| `Viewer` | Read operation summaries, alerts, GC reports, and drift reports. |
| `Operator` | Run non-break-glass operational actions such as recovery inspection. |
| `BreakGlassOperator` | Force-resolve InDoubt transactions with reason and traceability. |
| `Admin` | Change daemon schedules, retention policy, backend configuration, and RBAC grants. |
| `Auditor` | Read immutable audit history and export incident records. |

## Permission Matrix

| Action | Viewer | Operator | BreakGlassOperator | Admin | Auditor |
| --- | --- | --- | --- | --- | --- |
| List operation summaries | yes | yes | yes | yes | yes |
| List alerts | yes | yes | yes | yes | yes |
| Acknowledge alert | no | yes | yes | yes | no |
| Resolve alert | no | no | yes | yes | no |
| Suppress alert | no | no | yes | yes | no |
| Expire alert lifecycle state | no | no | no | yes | no |
| Read alert checkpoints | no | no | no | yes | yes |
| List InDoubt transactions | yes | yes | yes | yes | yes |
| Force-resolve transaction | no | no | yes | yes | no |
| Force-unlock session | no | no | future | future | no |
| Change retention policy | no | no | no | yes | no |
| Change daemon schedule | no | no | no | yes | no |
| Export audit history | no | no | no | yes | yes |

`ForceUnlock` remains unsupported until session identity, device ownership, and audit requirements are designed.

## Fail-Closed Rules

The product backend must fail closed for privileged operations:

- If authorization lookup fails, deny the operation.
- If product audit write fails before `force-resolve`, deny the operation.
- If product audit write fails after a transaction reaches `InDoubt`, emit a local `audit.write_failed` event and keep the transaction blocking.
- If alert delivery fails, do not advance the alert checkpoint.
- If alert lifecycle product audit write fails, do not update alert lifecycle state.
- If RBAC config is missing, default to read-only access at most.

Read-only local inspection may continue in degraded mode only when the operator explicitly chooses local JSONL mode.

## Migration Path

1. Keep JSONL local mode as the default development backend.
2. Add product-backed implementations of the existing traits.
3. Add config selection:

```json
{
  "operations_backend": {
    "mode": "product",
    "endpoint": "https://audit.example.internal",
    "tenant": "default"
  }
}
```

4. Run dual-write in staging: product backend plus local JSONL emergency copy.
5. Compare counts by action/result/device for the same interval.
6. Promote product backend when dual-write counts and incident replay match.
7. Keep JSONL export for support bundles and offline debugging.

## API Shape

The product API should expose:

- `GET /operations/summaries`
- `GET /operations/summaries/overview`
- `GET /operations/alerts`
- `GET /operations/alerts/overview`
- `POST /operations/alerts/{dedupe_key}/ack`
- `POST /operations/alerts/{dedupe_key}/resolve`
- `POST /operations/alerts/{dedupe_key}/suppress`
- `GET /transactions/in-doubt`
- `POST /transactions/{tx_id}/force-resolve`
- `GET /audit/events`

All write APIs require idempotency through `request_id`.

## Testing Requirements

Required tests before product backend promotion:

- Authorization denied for each privileged action.
- Missing reason rejects force-resolve.
- Audit write failure rejects force-resolve.
- Audit write failure rejects alert lifecycle writes.
- Audit write failure rejects local worker retention and schedule config writes.
- Alert delivery failure preserves checkpoint.
- Dual-write summary count parity.
- RBAC role matrix coverage.
- Audit export includes force-resolve operator and reason.
- Alert lifecycle history preserves operator, role, reason, request ID, and trace ID.

## Current Decision

Continue using JSONL local mode for no-real-switch development. Product audit/RBAC now has a local foundation for `force_resolve_transaction`, internal alert lifecycle actions, and local worker retention/schedule config changes. Build the database-backed product audit/RBAC backend behind existing traits once product storage and internal token lifecycle requirements are fixed.

External alert delivery is not part of the current product direction. Do not build webhook, enterprise IM, PagerDuty, email, or external retry adapters unless the product requirement changes. Alerts remain internal records queried through `aria-underlay-ops`, product APIs, and later UI. Current local alert lifecycle supports `open`, `acknowledged`, `resolved`, `suppressed`, and `expired`, with audit and RBAC on every operator action. Current local worker config admin supports audited retention and schedule config writes, but does not hot-reload a running daemon. Remaining product work is database-backed lifecycle/config storage plus product API/UI packaging.
