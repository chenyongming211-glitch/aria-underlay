# Product Worker Config Admin Implementation Plan — 2026-05-04

## Scope

Expose existing worker config admin operations through product API and HTTP
routes.

## Steps

1. Add product HTTP tests for schedule change.
   - Admin succeeds, config file changes, audit record is written.
   - Viewer is denied, config file remains unchanged, no audit record is
     written.

2. Add product request body types.
   - summary retention change
   - journal GC retention change
   - worker schedule change

3. Add `ProductOpsManager` methods that delegate to
   `WorkerConfigAdminManager`.

4. Add `ProductOpsApi` facade methods.

5. Add framework-neutral HTTP routes.

6. Update docs and run verification.
