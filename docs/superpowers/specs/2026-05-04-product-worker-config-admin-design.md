# Product Worker Config Admin Design — 2026-05-04

## Goal

Expose the existing worker configuration administration flow through the
product API boundary so operators can request retention and schedule changes
through the same identity, RBAC, and product audit path as other product
operations.

## Design

Reuse `WorkerConfigAdminManager` instead of duplicating mutation logic. Product
API request bodies contain only product-facing fields:

- config path
- reason
- target
- retention or schedule payload

The product envelope supplies request ID, trace ID, operator ID, and role. The
product manager converts that envelope into the existing worker admin request.

The HTTP router adds three POST routes:

- `/product/v1/worker-config/operation-summary-retention:change`
- `/product/v1/worker-config/journal-gc-retention:change`
- `/product/v1/worker-config/schedule:change`

RBAC remains unchanged:

- retention changes require `AdminAction::ChangeRetentionPolicy`
- schedule changes require `AdminAction::ChangeDaemonSchedule`

Product audit remains fail-closed. If audit append fails, the config file is
not changed.

## Non-Goals

- online daemon hot reload
- distributed config store
- product UI
- bypassing local config file validation

This package only makes the existing file-backed admin path reachable through
the product API contract.
