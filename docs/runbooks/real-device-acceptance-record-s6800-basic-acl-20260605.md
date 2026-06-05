# Real Device Acceptance Record: H3C S6800 Basic IPv4 ACL

## Summary

| Field | Value |
| --- | --- |
| Date | 2026-06-05 |
| Operator | Codex with user-approved lab resources |
| Repository commit SHA | `46f614065b2c2759112b2363411781cd60eccf86` |
| GitHub Actions run | `26990919003` |
| Adapter image | Local Python adapter from this workspace, real NETCONF mode |
| Probe artifact or branch | `real-domain-apply-probe-linux-x86_64-musl` from CI run `26990919003` |
| Device IP | `10.58.8.116` lab switch |
| Device model | H3C S6800-54QF |
| OS version | 7.1.070 Release 2612P06 |
| Secret ref | `local/s6800-116` |
| Test ACL | `2999` |
| Test ACL kind | `basic_ipv4` |
| Test ACL description | `aria-basic-acl` |
| Test ACL rule description | None; S6800 rejects `IPv4BasicRules/Rule/Description` |
| Idempotency key | `s6800-basic-acl-2999-apply-fixed-20260605` |

## Baseline

| Resource | Baseline value |
| --- | --- |
| Test ACL present before write | No; `display acl 2999` returned no ACL body |
| Existing IPv4 ACL ids | S6800 lab readback did not show ACL `2999` before the test |
| ACL binding present before write | No ACL binding was requested or created |

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

## ACL Acceptance

| Check | Result |
| --- | --- |
| Candidate ACL was absent before write | Passed |
| Candidate ACL was re-checked immediately before write | Passed by dry-run planning `CreateAcl(2999)` |
| Dry-run contained `CreateAcl` for test ACL | Passed; `CreateAcl(AclConfig { acl_id: 2999, kind: BasicIpv4, ... })` |
| Dry-run contained no `UpdateAcl` or `DeleteAcl` | Passed |
| Apply status | `SuccessWithWarning` |
| Transaction strategy | `RunningRollbackOnError` |
| tx_id | `65cd6a3e-603c-4b40-b251-bf36a086299a` |
| Apply verify_report status | `Passed` |
| Apply verify_report scoped evidence | `acl_count=1`; no VLAN, interface, ACL binding, or delete scope |
| Readback ACL result | `Basic IPv4 ACL 2999, 1 rule`, description `aria-basic-acl` |
| Readback ACL rule result | `rule 10 permit source 192.0.2.1 0` |
| Readback ACL rule description result | Not applicable; unsupported on this model/OS |
| Readback binding check | No ACL binding was requested |
| Cleanup command dry-run inspected | Yes; `delete IPv4 ACL 2999` with NETCONF delete payload |
| Cleanup result | `cleanup complete` |
| Cleanup readback result | `display acl 2999` returned no ACL body |

## Findings

- The first real apply attempt included `IPv4BasicRules/Rule/Description` and the device rejected it with NETCONF `unknown-element Description`.
- That failed attempt returned `InDoubt`, but immediate CLI readback confirmed ACL `2999` was absent, so no residual device config was left.
- The renderer now rejects H3C Basic IPv4 ACL rule descriptions before device write, preventing the same device-side failure path.
- The cleanup script now accepts numeric IPv4 ACL ids in `2000..3999`, so Basic ACL test ids can be restored with the same runbook command.

## Verdict

- [x] Passed with documented warning.

Notes:

```text
SuccessWithWarning is expected on this S6800 profile because the device has
writable-running and rollback-on-error but no candidate or confirmed-commit.
No passwords, private keys, raw running XML, or full session transcripts are
stored in this record.
```
