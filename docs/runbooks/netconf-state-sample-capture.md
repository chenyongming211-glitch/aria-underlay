# NETCONF State Sample Capture Runbook

## Purpose

Use this runbook when collecting real Huawei/H3C `get-config` running XML for state parser development. The goal is to validate real XML structure before any parser is marked `production_ready=True`.

This workflow is offline after capture. It uses `aria-underlay-state-parse` to validate redacted XML samples without connecting to production devices through the adapter.

## Scope

Use this runbook for:

- Huawei VRP running-state XML samples.
- H3C Comware running-state XML samples.
- VLAN and interface mode parser validation.
- Parser gap reproduction fixtures.

Do not use this runbook to:

- test config deployment;
- validate renderer output;
- store raw production configuration in git;
- promote a parser to production-ready without tests and review.

## Preconditions

Before capturing a sample, record:

- vendor: `huawei` or `h3c`;
- device model;
- OS family and version;
- NETCONF transport endpoint used for capture;
- whether the device is lab, staging, or production;
- capture date;
- operator;
- intended parser profile, for example `vrp8-state-fixture` or `comware7-state-fixture`.

Raw captures must be stored outside the repository. Commit only redacted, reduced samples.

## Capture With ncclient

Run from a trusted workstation that can reach the device over NETCONF:

```bash
cat >/tmp/capture-running.py <<'PY'
from ncclient import manager

host = "DEVICE_MANAGEMENT_IP"
username = "NETCONF_USERNAME"
password = "NETCONF_PASSWORD"

with manager.connect(
    host=host,
    port=830,
    username=username,
    password=password,
    hostkey_verify=False,
    look_for_keys=False,
    allow_agent=False,
    timeout=30,
) as session:
    reply = session.get_config(source="running")
    print(reply.data_xml)
PY
```

Save stdout outside git first:

```bash
mkdir -p /tmp/aria-underlay-netconf-samples
python3 /tmp/capture-running.py > /tmp/aria-underlay-netconf-samples/huawei-vrp8-raw.xml
```

## Optional Scoped Capture

If full running config is too large or too sensitive, collect a reduced subtree that still contains parser-relevant VLAN and interface sections.

Use the same XML structure as adapter scoped reads:

```xml
<filter type="subtree">
  <vlans>
    <vlan><vlan-id>100</vlan-id></vlan>
  </vlans>
  <interfaces>
    <interface><name>GE1/0/1</name></interface>
  </interfaces>
</filter>
```

The parser still needs representative access and trunk interfaces, including native VLAN and allowed VLAN fields when available.

## Redaction Checklist

Before a sample can enter `adapter-python/tests/fixtures/state_parsers/real_samples/`, redact or replace:

- management IP addresses and hostnames;
- usernames, passwords, keys, SNMP communities, AAA configuration, and certificates;
- serial numbers, MAC addresses, asset tags, and site names;
- customer VLAN names and interface descriptions;
- public IP addresses, routing peer addresses, AS numbers when customer-identifying, and tenant names;
- any free-form field that names a customer, site, circuit, person, or internal system.

Keep parser-relevant structure intact:

- XML namespaces;
- element names;
- VLAN ID shape;
- interface name format, with neutralized but realistic names;
- access/trunk/native/allowed VLAN fields;
- present vs absent optional fields.

Use neutral replacement values such as:

```text
CUSTOMER_A -> tenant-a
10.41.12.8 -> 192.0.2.8
core-uplink-to-bank -> core uplink
SN210235A9KD -> SERIAL-REDACTED
```

## Validate A Redacted Sample

Run summary first:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml adapter-python/tests/fixtures/state_parsers/real_samples/huawei/vrp8/sample.redacted.xml \
  --summary
```

Expected shape:

```json
{
  "fixture_verified": true,
  "interface_count": 2,
  "production_ready": false,
  "profile_name": "vrp8-state-fixture",
  "scope": {
    "full": true,
    "interface_names": [],
    "vlan_ids": []
  },
  "vendor": "huawei",
  "vlan_count": 2
}
```

Use pretty output when the summary looks wrong:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample.redacted.xml \
  --pretty
```

Use scope to check touched resources:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample.redacted.xml \
  --summary \
  --vlan 100 \
  --interface GE1/0/1
```

## Handling Parser Failures

Parser failures are expected while real profiles are being developed. Capture the stderr JSON:

```json
{
  "code": "NETCONF_STATE_PARSE_FAILED",
  "message": "NETCONF running state parser failed",
  "normalized_error": "state parse failed",
  "raw_error_summary": "missing required text: vlan/vlan-id",
  "retryable": false
}
```

When validation fails:

1. confirm the XML is redacted and can be shared inside the repo;
2. reduce the XML to the smallest structure that reproduces the failure;
3. save it under `real_samples/<vendor>/<profile>/`;
4. add or update a parser test before changing parser behavior;
5. keep `production_ready=False` until multiple real samples pass and the profile is reviewed.

## Promotion Criteria

A parser can be considered for `production_ready=True` only after:

- at least one redacted real sample per target vendor/profile is committed;
- fixture tests cover both success and failure cases from real XML;
- `aria-underlay-state-parse --summary` reports expected resource counts;
- driver/backend fixture integration remains green;
- production path still rejects unapproved parsers by default;
- reviewer explicitly accepts the parser profile evidence.

Successful parsing of one sample is not sufficient by itself.
