# Worker Reload Status Query Design

## Goal

Expose the worker daemon reload checkpoint through operator-facing query
surfaces so operators do not need to read JSON files by hand.

## Scope

This package is read-only. It does not change daemon reload behavior, write
product audit records, or add a product UI. It adds local CLI and product
API/HTTP query paths for the checkpoint created by the reload supervisor.

## Approach

Reuse `WorkerReloadCheckpoint` as the response body. Add a small request type
that points at the checkpoint file path, then expose it through:

- `aria-underlay-ops worker-reload-status --checkpoint-path <file>`
- `ProductOpsManager::get_worker_reload_status`
- `ProductOpsApi::get_worker_reload_status`
- `POST /product/v1/worker-reload/status:get`

Product reads pass through `AuthorizationPolicy` using a new read-only admin
action. The action is allowed for every assigned role, matching operation
summary and alert reads. Missing or corrupt checkpoint files fail closed with a
clear invalid-request error.

## Data Flow

The daemon remains the only writer of the checkpoint. Query paths only read and
deserialize the checkpoint. The product HTTP route accepts the checkpoint path
in JSON body, extracts the product session from headers, authorizes the read,
and returns `ProductApiResponse<WorkerReloadCheckpoint>`.

## Testing

Add no-real-switch tests:

- local CLI prints a checkpoint from disk.
- product HTTP viewer session can read the checkpoint.
- unassigned product operator is denied.
- missing checkpoint path fails closed.
