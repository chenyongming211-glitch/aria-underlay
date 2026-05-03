# Worker Deployment Ops Design

## Goal

Make the worker daemon safer to deploy without requiring a real switch by adding checked-in production deployment samples and an offline config preflight command.

## Scope

Included:

- A production-style worker daemon JSON config sample with absolute paths under `/var/lib/aria-underlay`.
- A systemd service sample that validates config before daemon startup.
- A tmpfiles.d sample that documents runtime directory ownership and permissions.
- `aria-underlay-ops check-worker-config` for offline structural and optional filesystem checks.
- Tests that parse the checked-in samples and exercise valid and invalid preflight cases.

Excluded:

- Online daemon reload.
- Host-specific package installation.
- Real switch sessions.
- External alert delivery.

## Design

The preflight lives in a focused Rust module under `src/worker/deployment.rs`. It reads `UnderlayWorkerDaemonConfig`, validates dependency rules and schedules, validates retention policies, and can optionally check whether required parent directories exist and are writable. The command never starts workers, opens adapter sessions, locks devices, edits config, or changes persistent state except for temporary write probes in directories being checked.

`strict_paths=false` is suitable for CI and config authoring because it checks semantics only. `strict_paths=true` is suitable for host startup because it verifies required directories exist and can be written by the service account.

## Failure Semantics

Preflight is fail-closed:

- JSON parse errors return an invalid report and non-zero CLI status.
- `operation_alert` without `operation_summary` is invalid.
- Any enabled schedule with `interval_secs=0` is invalid.
- Invalid summary or journal GC retention is invalid.
- In strict mode, missing or non-writable required directories are invalid.

The CLI prints a JSON report before returning failure so operators and service managers have machine-readable evidence.

## Deployment Samples

The systemd sample uses:

- `ExecStartPre=/usr/local/bin/aria-underlay-ops check-worker-config --worker-config-path /etc/aria-underlay/worker.json --strict-paths`
- `ExecStart=/usr/local/bin/aria-underlay-worker /etc/aria-underlay/worker.json`
- `User=aria-underlay`
- restricted write paths for `/var/lib/aria-underlay`, `/var/log/aria-underlay`, and `/run/aria-underlay`

The tmpfiles.d sample creates the directories needed by the production JSON sample with `aria-underlay` ownership.

## Testing

Tests cover:

- Checked-in deployment sample consistency.
- Strict preflight success with existing writable directories.
- Schedule validation without starting daemon workers.
- Strict path failure for missing directories.
- CLI success path for `check-worker-config --strict-paths`.

Local Rust tests may be unavailable on this workstation because `cargo` is not installed. GitHub Actions remains the Rust compile and test gate.
