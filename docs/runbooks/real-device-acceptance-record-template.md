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

## Baseline

| Resource | Baseline value |
| --- | --- |
| Test VLAN present before write | No |
| Access interface |  |
| Access original PVID |  |
| Trunk interface |  |
| Trunk original allowed VLANs |  |

## Access Acceptance

| Check | Result |
| --- | --- |
| Dry-run contained no delete ops |  |
| Dry-run summary |  |
| Apply status |  |
| Transaction strategy |  |
| tx_id |  |
| Readback VLAN result |  |
| Readback access result |  |
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
| Readback trunk result |  |
| Cleanup command dry-run inspected |  |
| Cleanup result |  |
| Cleanup readback result |  |

## Logs And Follow-Up

| Item | Value |
| --- | --- |
| Adapter log anomalies |  |
| Recoverable transactions after test |  |
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
