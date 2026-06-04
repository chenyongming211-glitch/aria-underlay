# Real Device Acceptance Record: H3C S6800 Access VLAN

## Summary

| Field | Value |
| --- | --- |
| Date | 2026-06-04 |
| Operator | Codex with user-approved lab resources |
| Repository commit SHA | `586215be1d0027048aec9f8f62a667fb10fb48db` |
| GitHub Actions run | `26942931511` |
| Adapter image | Local Python adapter from this workspace, real NETCONF mode |
| Probe artifact or branch | `real-domain-apply-probe-linux-x86_64-musl` from CI run `26942931511` |
| Device IP | `10.58.8.116` lab switch |
| Device model | H3C S6800-54QF |
| OS version | 7.1.070 Release 2612P06 |
| Secret ref | `local/s6800-116` |
| Test VLAN | `3333` |
| Test VLAN name | `aria-s6800-test` |
| Access interface | `Ten-GigabitEthernet1/0/1` / `XGE1/0/1` |
| Idempotency key | `s6800-real-device-access-3333-apply-001` |

## Baseline

| Resource | Baseline value |
| --- | --- |
| Test VLAN present before write | No; `display vlan 3333` returned `This VLAN does not exist.` |
| Access interface | `XGE1/0/1` |
| Access original state | `DOWN`, access mode, PVID `1` |
| Access original explicit config | `port link-mode bridge` only |
| Access original description | Empty |

## Transaction Preflight

| Check | Result |
| --- | --- |
| SSH CLI reachable | Yes |
| NETCONF reachable | Yes |
| Device model and OS captured by read-only command | H3C S6800-54QF, 7.1.070 Release 2612P06 |
| Recommended transaction strategy | `RunningRollbackOnError` |
| Candidate support | No |
| Validate support | Yes |
| Confirmed-commit support | No |
| Rollback-on-error support | Yes |
| Preflight status | `ready_for_scoped_write_acceptance` |

## Access Acceptance

| Check | Result |
| --- | --- |
| Dry-run contained no delete ops | Passed |
| Dry-run summary | `CreateVlan(3333)` and `UpdateInterface(XGE1/0/1 access vlan 1 -> 3333)` |
| Apply status | `SuccessWithWarning` |
| Transaction strategy | `RunningRollbackOnError` |
| tx_id | `2404e08e-5a04-43d8-ad59-110bc0d47a00` |
| Apply verify_report status | `Passed` |
| Apply verify_report scoped evidence | `vlan_count=1`, `interface_count=1`, no ACL or delete scope |
| Readback VLAN result | VLAN `3333` existed with name `aria-s6800-test` |
| Readback access result | `XGE1/0/1` was `DOWN`, access mode, PVID `3333` |
| Cleanup command dry-run inspected | Yes; restore access PVID `1`, then delete VLAN `3333` |
| Cleanup result | `cleanup complete` |
| Cleanup readback result | VLAN `3333` absent; `XGE1/0/1` restored to access PVID `1` with only `port link-mode bridge` |

## Notes

- The `SuccessWithWarning` result is expected for this model because it does not support candidate or confirmed-commit. The warning documents degraded atomicity under `RunningRollbackOnError`.
- During readback, VLAN `3333` appeared as tagged on `Ten-GigabitEthernet1/0/48` because that trunk permits all VLANs. After VLAN deletion, the test VLAN was absent.
- No passwords, private keys, raw running XML, or full session transcripts are stored in this record.

## Verdict

- [x] Passed with documented warning.
