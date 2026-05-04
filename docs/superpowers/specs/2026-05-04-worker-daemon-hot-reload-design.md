# Worker Daemon Hot Reload Design

## Goal

Add an online reload contract for `aria-underlay-worker` so audited worker
config changes can be picked up by a running daemon without requiring a full
process restart.

## Scope

This package covers daemon-side reload only. It does not add external alert
delivery, product UI, database-backed audit, or live changes to an already
running worker loop. Reload is implemented by replacing the worker runtime.

## Architecture

Use a supervisor around `UnderlayWorkerRuntime` rather than mutating live
worker intervals. The supervisor reads the JSON worker config, starts a
runtime, polls the config file for content changes, and handles each change as
an atomic candidate:

1. Read the changed config file.
2. Parse it as `UnderlayWorkerDaemonConfig`.
3. Validate dependencies and worker schedules before touching the current
   runtime.
4. If valid, stop the current runtime through its shutdown channel, wait for
   its report, build a fresh runtime from the new config, and start it.
5. If invalid, keep the current runtime running and persist a rejected reload
   checkpoint with the error.

This gives the daemon a stable long-term behavior: config mutation remains
file-backed and audited, while runtime adoption is explicit, observable, and
fail-closed.

## Config

Add an optional top-level `reload` section:

```json
{
  "reload": {
    "enabled": true,
    "poll_interval_secs": 5,
    "checkpoint_path": "var/aria-underlay/ops/worker-reload-checkpoint.json"
  }
}
```

If `reload` is absent or disabled, the existing daemon behavior is unchanged.
If enabled, `poll_interval_secs` must be greater than zero and
`checkpoint_path` must be present.

## Checkpoint

The checkpoint is a small JSON file written atomically after daemon startup,
successful reload, rejected reload, and shutdown. It includes:

- config path
- generation
- fingerprint of the adopted config
- status: `started`, `applied`, `rejected`, or `shutdown`
- timestamp
- optional error

Operators can read this file to know whether the running daemon has adopted
the latest config or rejected it.

## Error Handling

Initial invalid config still fails startup. Later invalid reload candidates do
not stop the daemon; they are rejected and the last valid runtime continues.
Checkpoint write failure is fail-closed for startup and successful reload
because otherwise operators cannot know what the daemon adopted.

## Testing

Add daemon-level tests with no real switch:

- reload applies a changed schedule and records a newer generation checkpoint.
- invalid changed config is rejected while the current runtime keeps running.
- deployment preflight rejects invalid reload config.
