# Active-passive HA runbook

## Scope

This runbook covers the minimal HA mode for the current release:

- One Rust Core process is active at a time.
- A standby Core process may be started on a second node, but it must not accept writes until it acquires the active lease.
- The system does not support active-active. `EndpointLockTable` is process-local and is not a distributed device lock.
- The Python adapter should run on the same node as its Core and bind to loopback, or the deployment must provide an external encrypted/authenticated tunnel.

## Shared State

Both HA nodes must see the same persistent state paths when failover is expected:

- transaction journal root
- desired shadow root
- observed shadow root, if drift audit state should survive failover
- operation summary/audit/alert files, if product and worker state should follow the active node
- transaction artifacts, if artifact retention or debugging depends on them
- active lease file path

Use a storage layer that gives atomic create and atomic rename semantics for these paths. Do not point two active Core processes at independent journal/shadow directories and expect failover to be correct.

## Active Lease

Use `ActiveLeaseConfig` with a lease file under the shared runtime directory, for example:

```text
/var/lib/aria-underlay/ha/active.lock
```

The active Core should be constructed through:

```rust
let active = service
    .activate_active_passive(ActiveLeaseConfig::new(
        "/var/lib/aria-underlay/ha/active.lock",
        "core-node-a",
    ))
    .await?;
```

Activation does three things in order:

1. Acquire the active lease with atomic file creation.
2. Start a heartbeat that refreshes the lease record.
3. Run `recover_pending_transactions()` before returning the active service wrapper.

The returned `ActivePassiveAriaUnderlayService` checks that the lease is still current before accepting service operations. If another process owns the lease, operations fail with `HA_LEASE_LOST` or `HA_LEASE_HELD`.

## Startup Order

1. Start the Python adapter on the active node with loopback binding, for example `127.0.0.1:50051`.
2. Start the Rust Core on node A with the shared journal/shadow paths and the active lease path.
3. Confirm startup recovery completed by inspecting the activation recovery report or operation logs.
4. Start node B as standby only if its entrypoint is wired to acquire the same lease before accepting writes.
5. Verify node B cannot become active while node A is healthy; this should fail with `HA_LEASE_HELD`.

## Failover

1. Stop node A or fence it at the infrastructure layer.
2. Wait for the active lease heartbeat to become stale according to the configured TTL, unless node A released the lease cleanly.
3. Start or promote node B with the same lease path and shared state paths.
4. Node B must acquire the lease and run startup recovery before accepting new apply requests.
5. Inspect recoverable or in-doubt transactions before resuming normal changes.

## Safety Rules

- Do not expose the adapter gRPC port on an untrusted network without an external TLS/mTLS tunnel or sidecar.
- Do not run two active Core instances against the same switches.
- Do not configure node A and node B with different journal or shadow roots.
- Do not manually delete a lease file while the old active process may still be running; fence or stop the old active first.
- Treat `InDoubt` recovery results as a manual intervention point before pushing new changes to affected devices.
