# Operator Operations Runbook

This runbook covers the local/offline operations entrypoint that does not require a real switch.

## Scope

Covered:

- Operation summary inspection.
- Attention-required operation filtering.
- Operation alert inspection.
- Internal alert lifecycle: acknowledge, resolve, suppress, and expire.
- Worker daemon config and schedules.
- Journal/artifact GC signal review.
- Drift audit signal review.
- InDoubt transaction review and force-resolve.

Not covered:

- Product audit database deployment.
- Product identity provider deployment.
- Real switch parser/renderer promotion.

External paging systems such as enterprise IM, Slack, email, PagerDuty, or webhook delivery are intentionally out of scope. Alerts stay inside Aria Underlay and are queried through CLI, later product APIs, and later UI.

## Local Files

The checked-in sample config is:

```text
docs/examples/underlay-worker-daemon.local.json
```

It uses these local paths:

| File or directory | Purpose |
| --- | --- |
| `var/aria-underlay/ops/operation-summaries.jsonl` | Append-only operation summaries generated from operator-facing events. |
| `var/aria-underlay/ops/operation-alerts.jsonl` | Append-only alerts generated from attention-required summaries. |
| `var/aria-underlay/ops/operation-alert-state.json` | Internal alert lifecycle state keyed by alert `dedupe_key`. |
| `var/aria-underlay/ops/operation-alert-checkpoint.json` | Dedupe checkpoint so alert delivery does not resend the same alert after restart. |
| `var/aria-underlay/ops/product-audit.jsonl` | Append-only product audit records for privileged operator actions in local mode. |
| `var/aria-underlay/journal` | File-backed transaction journal root. |
| `var/aria-underlay/artifacts` | Rollback/artifact root used by GC. |
| `var/aria-underlay/shadow/expected` | Expected shadow state for drift audit. |
| `var/aria-underlay/shadow/observed` | Observed shadow state for offline drift audit. |

Use site-specific absolute paths in production-like environments, for example `/var/lib/aria-underlay/...`.

## Start the Worker

Run the worker daemon with the sample config:

```bash
cargo run --bin aria-underlay-worker -- docs/examples/underlay-worker-daemon.local.json
```

Installed binary form:

```bash
aria-underlay-worker /etc/aria-underlay/worker.json
```

The daemon wires these workers when the corresponding config sections exist:

- `operation_summary.retention_schedule`: compacts local summary JSONL.
- `operation_alert.schedule`: emits local alerts from attention-required summaries.
- `journal_gc.schedule`: cleans terminal journal records and artifacts by retention policy.
- `drift_audit.schedule`: compares expected vs observed shadow stores.

If `operation_alert` is configured without `operation_summary`, startup fails closed because alerts are derived from operation summaries.

## Inspect Operation Summaries

Use `aria-underlay-ops` for operator-facing JSON output:

```bash
cargo run --bin aria-underlay-ops -- operation-summary \
  --operation-summary-path var/aria-underlay/ops/operation-summaries.jsonl
```

List records that need human attention:

```bash
cargo run --bin aria-underlay-ops -- list-operations \
  --operation-summary-path var/aria-underlay/ops/operation-summaries.jsonl \
  --attention-required \
  --limit 20
```

Useful filters:

```bash
--action transaction.in_doubt
--action drift.detected
--action audit.write_failed
--result in_doubt
--result failed
--device-id leaf-a
--tx-id tx-123
```

Interpretation:

- `attention_required=true`: an operator should inspect the record.
- `result=in_doubt`: transaction recovery could not prove final state.
- `result=failed`: operation failed and should be triaged.
- `action=audit.write_failed`: local summary persistence failed and the audit path needs attention.
- `action=drift.detected`: observed state differs from expected shadow state.

## Inspect Alerts

List critical alerts:

```bash
cargo run --bin aria-underlay-ops -- list-alerts \
  --operation-alert-path var/aria-underlay/ops/operation-alerts.jsonl \
  --alert-state-path var/aria-underlay/ops/operation-alert-state.json \
  --severity Critical \
  --limit 20
```

Print alert counts:

```bash
cargo run --bin aria-underlay-ops -- alert-summary \
  --operation-alert-path var/aria-underlay/ops/operation-alerts.jsonl \
  --alert-state-path var/aria-underlay/ops/operation-alert-state.json
```

Alert severity:

| Severity | Meaning |
| --- | --- |
| `Critical` | Transaction InDoubt, audit write failure, or failed/in-doubt result. |
| `Warning` | Attention-required condition that does not meet the critical rule. |

The checkpoint file only records delivered dedupe keys. Deleting it causes the alert worker to deliver existing attention-required summaries again.

## Manage Alert Lifecycle

`OperationAlert` records are immutable evidence. Operator actions are recorded in a separate lifecycle state file and written to product audit before state changes.

Lifecycle states:

| State | Meaning |
| --- | --- |
| `Open` | No operator lifecycle action has been recorded for the alert. |
| `Acknowledged` | An operator accepted triage ownership. |
| `Resolved` | A break-glass operator or admin marked the condition handled. |
| `Suppressed` | A break-glass operator or admin intentionally hid the alert from active triage. |
| `Expired` | An admin retired stale lifecycle state. |

Allowed transitions:

- `Open` -> `Acknowledged`, `Resolved`, `Suppressed`, or `Expired`.
- `Acknowledged` -> `Resolved`, `Suppressed`, or `Expired`.
- `Resolved`, `Suppressed`, and `Expired` are terminal.

RBAC:

| Action | Allowed roles |
| --- | --- |
| `ack-alert` | `Operator`, `BreakGlassOperator`, `Admin` |
| `resolve-alert` | `BreakGlassOperator`, `Admin` |
| `suppress-alert` | `BreakGlassOperator`, `Admin` |
| `expire-alert` | `Admin` |

Acknowledge an alert:

```bash
cargo run --bin aria-underlay-ops -- ack-alert \
  --alert-state-path var/aria-underlay/ops/operation-alert-state.json \
  --product-audit-path var/aria-underlay/ops/product-audit.jsonl \
  --dedupe-key "transaction.in_doubt|in_doubt|req-123|trace-123|tx-123|leaf-a" \
  --operator alice \
  --role Operator \
  --reason "investigating tx-123 recovery state"
```

Resolve after out-of-band verification:

```bash
cargo run --bin aria-underlay-ops -- resolve-alert \
  --alert-state-path var/aria-underlay/ops/operation-alert-state.json \
  --product-audit-path var/aria-underlay/ops/product-audit.jsonl \
  --dedupe-key "transaction.in_doubt|in_doubt|req-123|trace-123|tx-123|leaf-a" \
  --operator bob \
  --role BreakGlassOperator \
  --reason "validated transaction state and force-resolved tx-123"
```

If product audit cannot be written, lifecycle writes fail closed and the alert state file is not updated.

## Triage GC

GC completion summaries use:

```text
action=journal.gc_completed
result=completed
```

Important fields:

- `journals_deleted`
- `journals_retained`
- `artifacts_deleted`
- `deleted_total`
- `journal_deleted_tx_ids`
- `artifact_deleted_refs`

GC never deletes `InDoubt` transactions automatically. If GC deletes nothing, that is normal when records are newer than retention or are not terminal.

## Triage Drift

Drift completion summaries use:

```text
action=drift.audit_completed
```

Device-specific drift records use:

```text
action=drift.detected
result=drift_detected
```

Important fields:

- `audited_device_count`
- `drifted_device_count`
- `drifted_devices`
- `finding_count`
- `first_drift_type`
- `first_path`

Current behavior is detect-only. `AutoReconcile` remains fail-closed until a separate approval and rollback design exists.

## Triage InDoubt Transactions

List current InDoubt transactions:

```bash
cargo run --bin aria-underlay-ops -- list-in-doubt \
  --journal-root var/aria-underlay/journal
```

Filter by device:

```bash
cargo run --bin aria-underlay-ops -- list-in-doubt \
  --journal-root var/aria-underlay/journal \
  --device-id leaf-a
```

Use force-resolve only after an operator has verified device state out of band:

```bash
cargo run --bin aria-underlay-ops -- force-resolve \
  --journal-root var/aria-underlay/journal \
  --operation-summary-path var/aria-underlay/ops/operation-summaries.jsonl \
  --tx-id tx-123 \
  --operator alice \
  --reason "verified running config on leaf-a and leaf-b" \
  --break-glass
```

Force-resolve is a manual audit action. It does not push configuration and does not prove device state. It only marks the transaction as administratively resolved so later transactions are no longer blocked by the old InDoubt record.

## Response Checklist

When an alert appears:

1. Run `alert-summary` to see severity counts.
2. Run `list-alerts --severity Critical --limit 20`.
3. Run `ack-alert` once someone starts investigation.
4. For `transaction.in_doubt`, run `list-in-doubt`.
5. Verify actual device or fake-adapter state through the relevant runbook.
6. Use `force-resolve` only with a concrete reason and operator identity.
7. Run `resolve-alert` only after the underlying condition has been handled.
8. Keep the alert, lifecycle, summary, and product audit files for incident review until retention policy or product audit backend owns the record.

## Product Boundary

Local JSONL mode is intentionally simple and auditable, but it is not the final product operations backend. Production should replace or wrap the same summary and alert traits with:

- durable database storage,
- operator identity,
- RBAC,
- immutable audit records,
- searchable UI/API.

The design boundary is recorded in:

```text
docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md
```
