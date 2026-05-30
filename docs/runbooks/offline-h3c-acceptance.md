# Offline H3C Acceptance Runner

This runner gives CI a repeatable H3C command-surface acceptance signal when no
real switch is available.

It does not replace the real-device acceptance runbook. It verifies that the
current H3C renderer and mock NETCONF backend can exercise the supported command
surface end to end:

- VLAN create and VLAN description
- access interface mode and interface description
- trunk allowed VLANs
- IPv4 advanced ACL rules
- ACL rule description
- ACL interface binding
- explicit delete VLAN, delete ACL, and delete ACL binding cleanup intents

Run locally from the repository root after installing the adapter package:

```bash
python -m pip install -e "adapter-python[test]"
aria-underlay-h3c-offline-acceptance --pretty
```

The command prints a machine-readable JSON report to stdout and a human-readable
summary to stderr. CI also writes both forms to an artifact:

```bash
aria-underlay-h3c-offline-acceptance \
  --pretty \
  --json-report report.json \
  --summary summary.txt
```

Acceptance passes only when every scenario renders valid H3C Comware XML and
then completes mock NETCONF dry-run, prepare, commit, final-confirm, and verify.
