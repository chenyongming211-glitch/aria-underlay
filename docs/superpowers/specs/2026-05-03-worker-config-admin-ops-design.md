# Worker Config Admin Ops Design

## Goal

Add audited, RBAC-protected local operations for changing worker daemon retention and schedule settings without requiring a real switch.

## Scope

In scope:

- Update the JSON worker daemon config file through `aria-underlay-ops`.
- Change operation summary retention policy.
- Change journal GC retention policy.
- Change worker schedules for operation summary compaction, operation alert delivery, journal GC, and drift audit.
- Require RBAC authorization and product audit before writing config changes.

Out of scope:

- Online daemon hot reload.
- Product database storage.
- Product API/UI.
- Real switch validation.

## Design

The worker daemon already reads `UnderlayWorkerDaemonConfig` from JSON and wires workers from that config. This package adds a dedicated `WorkerConfigAdminManager` that owns safe config mutation:

1. Validate request fields and target-specific policy/schedule values.
2. Authorize with existing `AuthorizationPolicy`.
3. Append a product audit pre-record.
4. Load the current worker config.
5. Apply the requested patch.
6. Persist the full config with atomic write.

The manager is deliberately separate from `UnderlayWorkerDaemon` because it changes desired daemon configuration, not a running worker instance.

## RBAC

Use existing `AdminAction` values:

| Operation | `AdminAction` | Allowed role |
| --- | --- | --- |
| operation summary retention | `ChangeRetentionPolicy` | `Admin` |
| journal GC retention | `ChangeRetentionPolicy` | `Admin` |
| worker schedule | `ChangeDaemonSchedule` | `Admin` |

`Viewer`, `Operator`, `BreakGlassOperator`, and `Auditor` are denied for these write operations.

## Audit

Product audit records are written before config mutation. If audit append fails, the config file must not change.

Audit actions:

- `daemon.retention_change_requested`
- `daemon.schedule_change_requested`

Records include:

- `operator_id`
- `role`
- `reason`
- `config_path`
- `target`
- changed values in `fields`

## CLI

Add these commands:

```bash
aria-underlay-ops set-summary-retention \
  --worker-config-path <file> \
  --product-audit-path <file> \
  --operator <name> \
  --role Admin \
  --reason <text> \
  [--max-records <n>] \
  [--max-bytes <n>] \
  [--max-rotated-files <n>]
```

```bash
aria-underlay-ops set-gc-retention \
  --worker-config-path <file> \
  --product-audit-path <file> \
  --operator <name> \
  --role Admin \
  --reason <text> \
  --committed-days <n> \
  --rolled-back-days <n> \
  --failed-days <n> \
  --rollback-artifact-days <n> \
  --max-artifacts-per-device <n>
```

```bash
aria-underlay-ops set-worker-schedule \
  --worker-config-path <file> \
  --product-audit-path <file> \
  --operator <name> \
  --role Admin \
  --reason <text> \
  --target operation-summary-retention|operation-alert|journal-gc|drift-audit \
  --interval-secs <n> \
  --run-immediately true|false
```

If the target section is missing from config, the command fails closed instead of silently creating a partial daemon config.

## Testing

Tests cover:

- Admin can update summary retention and product audit records the request.
- Non-admin cannot update schedule and config stays unchanged.
- Product audit write failure blocks config mutation.
- CLI updates worker config and writes audit.
- Invalid schedule interval zero is rejected before file mutation.
