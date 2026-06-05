# Real Device Acceptance Record: H3C S5560 Basic IPv4 ACL And ACL Binding

## Summary

| Field | Value |
| --- | --- |
| Date | 2026-06-05 |
| Operator | Codex with user-approved lab resources |
| Repository commit SHA | `f0c156cd0ad1d620765d3b17a6bcb691357c620c` |
| GitHub Actions run | Basic ACL probe artifact from `26991348307`; ACL binding probe artifact from `26999407466` |
| Adapter image | Local Python adapter from this workspace, real NETCONF mode |
| Device IP | `10.58.8.120` lab switch |
| Device model | H3C S5560 |
| OS version | 7.1.070 |
| Secret ref | `local/s5560-120` |
| Basic test ACL | `2999` |
| Binding test ACL | `3999` |
| Binding interface | `GigabitEthernet1/0/30` / `GE1/0/30` |
| Binding direction | `inbound` |

## Baseline

| Resource | Baseline value |
| --- | --- |
| Basic ACL `2999` present before write | No; `display acl 2999` returned no ACL body |
| Advanced ACL `3999` present before binding write | No; `display acl 3999` returned no ACL body |
| Binding interface | `GigabitEthernet1/0/30` |
| Binding interface operational state | `DOWN`, access mode, PVID `1` |
| Binding interface explicit config | `port link-mode bridge` only |
| Existing packet-filter binding | None on `GigabitEthernet1/0/30` inbound |

## Transaction Preflight

| Check | Result |
| --- | --- |
| SSH CLI reachable | Yes |
| NETCONF reachable | Yes |
| Device model and OS captured by read-only command | H3C S5560, 7.1.070 |
| Recommended transaction strategy | `RunningRollbackOnError` |
| Candidate support | No |
| Validate support | Yes |
| Confirmed-commit support | No |
| Rollback-on-error support | Yes |
| Preflight status | `ready_for_scoped_write_acceptance` |

## Basic IPv4 ACL Acceptance

| Check | Result |
| --- | --- |
| Candidate ACL was absent before write | Passed |
| Candidate ACL was re-checked immediately before write | Passed by dry-run planning `CreateAcl(2999)` |
| Dry-run contained no `UpdateAcl` or `DeleteAcl` | Passed |
| Apply status | `SuccessWithWarning` |
| Transaction strategy | `RunningRollbackOnError` |
| tx_id | `f01adf1a-14f8-40b1-9267-89ace9cecc9d` |
| Apply verify_report status | `Passed` |
| Apply verify_report scoped evidence | `acl_count=1`; no VLAN, interface, ACL binding, or delete scope |
| Readback ACL result | `Basic IPv4 ACL 2999, 1 rule`, description `aria-basic-acl-s5560` |
| Readback ACL rule result | `rule 10 permit source 192.0.2.2 0` |
| Readback ACL rule description result | Not applicable; rule description left unset |
| Cleanup command dry-run inspected | Yes; `delete IPv4 ACL 2999` with NETCONF delete payload |
| Cleanup result | `cleanup complete` |
| Cleanup readback result | `display acl 2999` returned no ACL body; NETCONF scoped ACL readback returned `[]` |

## ACL Binding Acceptance

| Check | Result |
| --- | --- |
| Candidate ACL was absent before write | Passed |
| Candidate binding target was empty before write | Passed |
| Dry-run summary | `CreateAcl(3999, AdvancedIpv4)` and `CreateAclBinding(GE1/0/30 inbound -> 3999)` |
| Dry-run contained no update/delete ops | Passed |
| Apply status | `SuccessWithWarning` |
| Transaction strategy | `RunningRollbackOnError` |
| tx_id | `6c1c9f17-15f7-4e5c-85aa-94cb1566cef7` |
| Apply verify_report status | `Passed` |
| Apply verify_report scoped evidence | `acl_count=1`, `acl_binding_count=1`; no VLAN, interface, or delete scope |
| Readback ACL result | `Advanced IPv4 ACL 3999, 1 rule`, description `aria-acl-binding-s5560` |
| Readback ACL rule result | `rule 10 permit ip source 192.0.2.3 0 destination 198.51.100.3 0` |
| Readback binding result | `packet-filter 3999 inbound` on `GigabitEthernet1/0/30` |
| NETCONF parser readback | ACL `3999` and inbound binding on `GigabitEthernet1/0/30` matched desired state |
| Cleanup command dry-run inspected | Yes; unbind ACL first, then delete IPv4 ACL `3999` |
| Cleanup result | `cleanup complete` |
| Cleanup readback result | ACL `3999` absent, packet-filter binding absent, interface restored to `port link-mode bridge` only |

## Findings

- An initial adapter launch accidentally used fake mode and reported confirmed-commit behavior that the real S5560 does not support. CLI and NETCONF readback caught the mismatch, so that attempt was not accepted as real-device evidence.
- The real S5560 NETCONF capability set is writable-running plus validate and rollback-on-error; it has no candidate and no confirmed-commit. `SuccessWithWarning` is therefore expected.
- The tested S5560 profile rejects ACL rule `Description` in the Basic ACL and Advanced ACL packet-filter binding paths. The accepted runs kept rule description unset and validated the ACL group description instead.
- The `real_domain_apply_probe` now accepts canonical interface aliases when checking requested ACL binding creates, so `GigabitEthernet1/0/30` and `GE1/0/30` no longer create a false dry-run rejection.
- Binding-only verify now allows a scoped interface to exist when the interface is included only to read `PfilterApply`; the interface itself is not interpreted as a desired interface delete.

## Verdict

- [x] Basic IPv4 ACL passed with documented warning.
- [x] IPv4 advanced ACL packet-filter binding passed with documented warning.

Notes:

```text
No passwords, private keys, raw running XML, or full session transcripts are
stored in this record. The device was cleaned up after both write cases.
```
