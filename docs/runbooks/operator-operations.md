# Operator Operations Runbook

This runbook covers the local/offline operations entrypoint that does not require a real switch.

## Scope

Covered:

- Operation summary inspection.
- Attention-required operation filtering.
- Operation alert inspection.
- Internal alert lifecycle: acknowledge, resolve, suppress, and expire.
- Worker daemon config, retention, and schedule changes.
- Worker deployment samples and offline config preflight.
- Journal/artifact GC signal review.
- Drift audit signal review.
- InDoubt transaction review and force-resolve.

Not covered:

- Product audit database deployment.
- Internal token lifecycle tooling and product audit database deployment.
- Real switch parser/renderer promotion.

External paging systems such as enterprise IM, Slack, email, PagerDuty, or webhook delivery are intentionally out of scope. Alerts stay inside Aria Underlay and are queried through CLI, later product APIs, and later UI.

## Local Files

The checked-in sample config is:

```text
docs/examples/underlay-worker-daemon.local.json
docs/examples/underlay-worker-daemon.production.json
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

## Deployment Samples

Checked-in deployment samples are:

```text
docs/examples/underlay-worker-daemon.production.json
docs/examples/systemd/aria-underlay-worker.service
docs/examples/tmpfiles.d/aria-underlay.conf
```

The production JSON sample uses `/var/lib/aria-underlay` for state, journal, artifacts, summaries, alerts, and drift shadow stores. The tmpfiles.d sample creates those directories with `aria-underlay` ownership. The systemd sample runs a config preflight before daemon startup and restricts daemon writes to:

```text
/var/lib/aria-underlay
/var/log/aria-underlay
/run/aria-underlay
```

These files are deployment examples, not a package installer. Site packaging still owns user creation, binary placement, `/etc/aria-underlay/worker.json` installation, service enablement, log policy, and host-level disk quotas.

## Check Worker Config

Run preflight without starting daemon workers:

```bash
cargo run --bin aria-underlay-ops -- check-worker-config \
  --worker-config-path docs/examples/underlay-worker-daemon.production.json
```

Run strict host checks before service startup:

```bash
aria-underlay-ops check-worker-config \
  --worker-config-path /etc/aria-underlay/worker.json \
  --strict-paths
```

`check-worker-config` prints a JSON report with:

| Field | Meaning |
| --- | --- |
| `valid` | `true` only when no errors were found. |
| `strict_paths` | Whether filesystem directory and write probes were enabled. |
| `errors` | Fail-closed reasons such as invalid schedules or missing directories. |
| `warnings` | Non-blocking operator notes, such as a config with no enabled worker sections. |
| `checked_paths` | Files and directories considered by preflight. |

Preflight validates:

- JSON config parsing.
- `operation_alert` requires `operation_summary`.
- Worker schedule `interval_secs` must be greater than zero.
- Operation summary retention must have positive limits when set.
- Journal GC retention must keep `max_artifacts_per_device` greater than zero.
- In strict mode, required directories must exist and be writable by the current user.

Preflight does not open NETCONF sessions, lock devices, edit candidate config, run GC, run drift audit, deliver alerts, or compact summaries.

## Start the Worker

Run the worker daemon with the sample config:

```bash
cargo run --bin aria-underlay-worker -- docs/examples/underlay-worker-daemon.local.json
```

Installed binary form:

```bash
aria-underlay-worker /etc/aria-underlay/worker.json
```

When using the systemd sample, startup runs:

```bash
aria-underlay-ops check-worker-config --worker-config-path /etc/aria-underlay/worker.json --strict-paths
```

If preflight fails, the daemon is not started.

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

## Change Worker Config

Worker config admin commands update the JSON config file. A running daemon adopts
the change only when its top-level `reload.enabled` setting is true.

All config writes require:

- `--operator`
- `--role Admin`
- `--reason`
- `--product-audit-path`

Product audit is written before the config file is mutated. If product audit cannot be written, the config file is not changed.

Change operation summary retention:

```bash
cargo run --bin aria-underlay-ops -- set-summary-retention \
  --worker-config-path /etc/aria-underlay/worker.json \
  --product-audit-path var/aria-underlay/ops/product-audit.jsonl \
  --operator admin-a \
  --role Admin \
  --reason "reduce local summary retention for disk pressure" \
  --max-records 10000 \
  --max-bytes 10485760 \
  --max-rotated-files 5
```

Change journal GC retention:

```bash
cargo run --bin aria-underlay-ops -- set-gc-retention \
  --worker-config-path /etc/aria-underlay/worker.json \
  --product-audit-path var/aria-underlay/ops/product-audit.jsonl \
  --operator admin-a \
  --role Admin \
  --reason "align local journal retention with incident policy" \
  --committed-days 30 \
  --rolled-back-days 30 \
  --failed-days 90 \
  --rollback-artifact-days 30 \
  --max-artifacts-per-device 50
```

Change a worker schedule:

```bash
cargo run --bin aria-underlay-ops -- set-worker-schedule \
  --worker-config-path /etc/aria-underlay/worker.json \
  --product-audit-path var/aria-underlay/ops/product-audit.jsonl \
  --operator admin-a \
  --role Admin \
  --reason "slow down drift audits during maintenance" \
  --target drift-audit \
  --interval-secs 600 \
  --run-immediately false
```

Schedule targets:

| Target | Config section |
| --- | --- |
| `operation-summary-retention` | `operation_summary.retention_schedule` |
| `operation-alert` | `operation_alert.schedule` |
| `journal-gc` | `journal_gc.schedule` |
| `drift-audit` | `drift_audit.schedule` |

If the target section is absent from the config file, the command fails closed instead of creating a partial config.

## Worker Daemon Reload

Reload is configured in the worker JSON:

```json
{
  "reload": {
    "enabled": true,
    "poll_interval_secs": 5,
    "checkpoint_path": "var/aria-underlay/ops/worker-reload-checkpoint.json"
  }
}
```

When enabled, `aria-underlay-worker` polls the config file. A changed config is
parsed and validated before the current runtime is touched. Valid changes stop
the current runtime, start a fresh runtime from the new config, and write a
checkpoint with `status=applied`. Invalid changes are rejected, the old runtime
continues, and the checkpoint records `status=rejected` plus the validation
error.

Checkpoint states:

| State | Meaning |
| --- | --- |
| `started` | Daemon started under reload supervision and adopted generation 1. |
| `applied` | A changed config was validated and adopted. |
| `rejected` | A changed config was invalid; the previous runtime is still active. |
| `shutdown` | Daemon stopped after graceful shutdown. |

Reload supervisor settings are process-lifecycle settings. Changing
`reload.poll_interval_secs` or `reload.checkpoint_path` in the config should be
treated as a restart-required deployment change.

Read the current checkpoint through the local ops CLI:

```bash
cargo run --bin aria-underlay-ops -- worker-reload-status \
  --checkpoint-path var/aria-underlay/ops/worker-reload-checkpoint.json
```

The same checkpoint is available through the product HTTP route
`POST /product/v1/worker-reload/status:get` with body:

```json
{
  "checkpoint_path": "var/aria-underlay/ops/worker-reload-checkpoint.json"
}
```

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

The first product-facing Rust operation boundary is `ProductOpsManager` in `src/api/product_ops.rs`. The first handler-facing facade is `ProductOpsApi` in `src/api/product_api.rs`. The framework-neutral HTTP route contract is `ProductHttpRouter` in `src/api/product_http.rs`. The first local listener adapter is `ProductHttpServer` in `src/api/product_http_server.rs`, exposed by the `aria-underlay-product-api` binary. The first identity boundary is `product_identity` in `src/api/product_identity.rs`.

`ProductOpsApi` currently accepts a typed `ProductApiRequest<T>` envelope:

- `request_id`
- optional `trace_id`
- headers
- typed body

There are now two session extractors:

| Extractor | Intended use |
| --- | --- |
| `HeaderProductSessionExtractor` | Local/mock contract tests only. |
| `BearerTokenProductSessionExtractor` | Internal product API route wiring using the configured bearer-token verifier. |

The local/mock header extractor reads:

| Header | Meaning |
| --- | --- |
| `x-aria-operator-id` | Operator identity for local product API contract tests. |
| `x-aria-role` | One of `Viewer`, `Operator`, `BreakGlassOperator`, `Admin`, or `Auditor`. |

The bearer extractor reads:

| Header | Meaning |
| --- | --- |
| `Authorization: Bearer <token>` | Token passed to `ProductIdentityVerifier`. |

`StaticProductIdentityVerifier` is deterministic local/offline infrastructure. It maps bearer tokens to normalized principals with `operator_id`, role, optional issuer, optional subject, optional session ID, and optional expiry. Missing, malformed, unknown, and expired tokens fail closed before RBAC or product audit export runs.

Product decision: this is an internal system. SSO, OIDC, JWT, JWKS, refresh tokens, and browser sessions are intentionally out of scope for this repository. The packaged identity model is internal bearer tokens configured in `static_tokens`, optionally protected by a site-local ingress, firewall rule, or operator network boundary. Token creation, rotation, and revocation remain an operational process until a first-party product audit database and internal identity store are designed.

The product API config rejects unknown fields, so historical `jwt_jwks` or `jwt_jwks_file` settings fail at startup instead of being silently ignored.

`ProductHttpRouter` currently defines these product HTTP routes, and `ProductHttpServer` can serve them over a local HTTP/1.1 listener:

| Method | Path | Body | Success response |
| --- | --- | --- | --- |
| `POST` | `/product/v1/operations/summaries:query` | `ListOperationSummariesRequest` JSON | `ProductApiResponse<ListOperationSummariesResponse>` |
| `POST` | `/product/v1/product-audit:export` | `ExportProductAuditRequest` JSON | `ProductApiResponse<ExportProductAuditResponse>` |
| `POST` | `/product/v1/worker-config/operation-summary-retention:change` | `ProductChangeSummaryRetentionRequest` JSON | `ProductApiResponse<WorkerConfigAdminResponse>` |
| `POST` | `/product/v1/worker-config/journal-gc-retention:change` | `ProductChangeJournalGcRetentionRequest` JSON | `ProductApiResponse<WorkerConfigAdminResponse>` |
| `POST` | `/product/v1/worker-config/schedule:change` | `ProductChangeWorkerScheduleRequest` JSON | `ProductApiResponse<WorkerConfigAdminResponse>` |
| `POST` | `/product/v1/worker-reload/status:get` | `ProductGetWorkerReloadStatusRequest` JSON | `ProductApiResponse<WorkerReloadCheckpoint>` |
| `POST` | `/product/v1/status:bundle` | `ProductStatusBundleRequest` JSON | `ProductApiResponse<ProductStatusBundleResponse>` |

Required HTTP headers:

| Header | Meaning |
| --- | --- |
| `x-aria-request-id` | Operator-visible request ID. |
| `Authorization` | `Bearer <token>` when using `BearerTokenProductSessionExtractor`. |

Optional HTTP headers:

| Header | Meaning |
| --- | --- |
| `x-aria-trace-id` | Cross-service trace ID; defaults to request ID in error responses when omitted. |

HTTP errors are JSON. Authentication failures return `401` plus `www-authenticate: Bearer`, invalid requests return `400`, RBAC denial returns `403`, unknown paths return `404`, known paths with the wrong method return `405` plus `allow: POST`, and audit/internal failures return `500`.

The local listener adds transport-level guardrails before route dispatch:

| Guardrail | Behavior |
| --- | --- |
| HTTP version | Accepts HTTP/1.1 request lines. |
| request body | Requires fixed `Content-Length` semantics; unsupported transfer encodings return JSON `400`. |
| body size | `max_body_bytes` defaults to 1 MiB and oversized bodies return JSON `413`. |
| connection lifecycle | One request per connection; responses include `connection: close`. |

Start the local product API with:

```bash
aria-underlay-product-api docs/examples/product-api.local.json
```

The checked-in static-token sample binds to `127.0.0.1:8088` and uses static local bearer tokens.

Production packaging uses:

```bash
aria-underlay-product-api /etc/aria-underlay/product-api.json
```

Use `docs/examples/product-api.production.json` as the starting point. It sets `deployment_mode` to `production_ingress`, binds the product API to loopback, uses `/var/lib/aria-underlay/ops` for state, and requires internal bearer tokens through `static_tokens`. TLS, client authentication, ingress rate limiting, and proxy-header policy belong in the production ingress or local host policy, not inside the current HTTP listener.

Current product boundary behavior:

| Operation | RBAC action | Audit behavior |
| --- | --- | --- |
| List operation summaries | `ListOperationSummaries` | Read-only; no product audit record in this package. |
| Get worker reload status | `GetWorkerReloadStatus` | Read-only; no product audit record in this package. |
| Get product status bundle | `GetProductStatusBundle` | Read-only; no product audit record in this package. |
| Export product audit history | `ExportAuditHistory` | Writes `product_audit.export_requested` before returning records. |
| Change worker retention policy | `ChangeRetentionPolicy` | Writes `daemon.retention_change_requested` before changing config. |
| Change worker schedule | `ChangeDaemonSchedule` | Writes `daemon.schedule_change_requested` before changing config. |

Audit export is fail-closed. If the export action cannot be appended to product audit, no audit records are returned.

Worker config mutation is also fail-closed. If audit append, authorization, validation, or config parsing fails, the config file is not changed. These routes update the configured worker JSON file. A running daemon adopts the change only when reload supervision is enabled in that worker config.

Product API systemd packaging is checked in at:

```text
docs/examples/systemd/aria-underlay-product-api.service
docs/examples/tmpfiles.d/aria-underlay.conf
```

Install-time directory ownership should keep `/etc/aria-underlay` owned by `root:aria-underlay` with mode `0750`; runtime state remains under `/var/lib/aria-underlay`, logs under `/var/log/aria-underlay`, and runtime files under `/run/aria-underlay`.

Still missing from the product layer:

- Internal token lifecycle and rotation tooling, including audit-friendly replacement of `static_tokens`.
- Production TLS/ingress implementation and hardening outside the local listener.
- product UI.

The design boundary is recorded in:

```text
docs/superpowers/specs/2026-05-03-product-audit-rbac-design.md
docs/superpowers/specs/2026-05-03-product-ops-rbac-boundary-design.md
docs/superpowers/specs/2026-05-03-product-api-routing-skeleton-design.md
docs/superpowers/specs/2026-05-03-product-http-routing-design.md
docs/superpowers/specs/2026-05-04-product-session-identity-boundary-design.md
docs/superpowers/specs/2026-05-04-product-http-listener-design.md
docs/superpowers/specs/2026-05-04-product-internal-identity-scope-design.md
docs/superpowers/specs/2026-05-04-product-worker-config-admin-design.md
```
