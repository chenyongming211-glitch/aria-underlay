# Real Device Acceptance Record

Use one record per device/model acceptance run. Do not include passwords,
private keys, session tokens, or full secrets.

## Summary

| Field | Value |
| --- | --- |
| Date |  |
| Operator |  |
| Repository commit SHA |  |
| GitHub Actions run |  |
| Adapter image |  |
| Probe artifact or branch |  |
| Device IP |  |
| Device model |  |
| Secret ref |  |
| Test VLAN |  |
| Test VLAN description |  |
| Test ACL |  |
| Test ACL description |  |
| Test ACL rule description |  |
| Test ACL binding interface |  |
| Test ACL binding direction |  |

## Baseline

| Resource | Baseline value |
| --- | --- |
| Test VLAN present before write | No |
| Access interface |  |
| Access original PVID |  |
| Access original description |  |
| Trunk interface |  |
| Trunk original allowed VLANs |  |
| Trunk original description |  |
| Existing IPv4 advanced ACL ids |  |
| Test ACL present before write | No |
| ACL binding present before write | No |

## Access Acceptance

| Check | Result |
| --- | --- |
| Dry-run contained no delete ops |  |
| Dry-run summary |  |
| Apply status |  |
| Transaction strategy |  |
| tx_id |  |
| Readback VLAN result |  |
| Readback VLAN description result |  |
| Readback access result |  |
| Readback access description result |  |
| Cleanup command dry-run inspected |  |
| Cleanup result |  |
| Cleanup readback result |  |

## Trunk Acceptance

| Check | Result |
| --- | --- |
| Dry-run contained no delete ops |  |
| Dry-run summary |  |
| Apply status |  |
| Transaction strategy |  |
| tx_id |  |
| Readback VLAN result |  |
| Readback VLAN description result |  |
| Readback trunk result |  |
| Readback trunk description result |  |
| Cleanup command dry-run inspected |  |
| Cleanup result |  |
| Cleanup readback result |  |

## ACL Acceptance

| Check | Result |
| --- | --- |
| Candidate ACL was absent before write |  |
| Candidate ACL was re-checked immediately before write |  |
| Dry-run contained `CreateAcl` for test ACL |  |
| Dry-run contained no `UpdateAcl` or `DeleteAcl` |  |
| Apply status |  |
| Transaction strategy |  |
| tx_id |  |
| Readback ACL result |  |
| Readback ACL rule result |  |
| Readback ACL rule description result |  |
| Readback binding check |  |
| Cleanup command dry-run inspected |  |
| Cleanup result |  |
| Cleanup readback result |  |

## ACL Binding Acceptance

| Check | Result |
| --- | --- |
| Candidate ACL was absent before write |  |
| Candidate ACL was re-checked immediately before write |  |
| Binding interface/direction had no existing IPv4 ACL binding |  |
| Dry-run contained `CreateAcl` for test ACL |  |
| Dry-run contained `CreateAclBinding` for selected interface/direction |  |
| Dry-run contained no ACL or binding update/delete |  |
| Apply status |  |
| Transaction strategy |  |
| tx_id |  |
| Readback ACL result |  |
| Readback binding result |  |
| Cleanup dry-run showed unbind before ACL delete |  |
| Cleanup result |  |
| Cleanup readback result |  |

## Logs And Follow-Up

| Item | Value |
| --- | --- |
| Adapter log anomalies |  |
| Recoverable transactions after test |  |
| Test ACL binding remains after cleanup |  |
| Manual changes required |  |
| Open follow-up issue or PR |  |

## Verdict

- [ ] Passed.
- [ ] Passed with documented warning.
- [ ] Failed and cleaned up.
- [ ] Failed and requires manual restoration.

Notes:

```text

```
