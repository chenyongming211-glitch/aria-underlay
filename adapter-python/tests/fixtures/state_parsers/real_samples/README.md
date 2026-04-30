# Real NETCONF State Parser Samples

This directory is reserved for redacted real `get-config` running XML samples used to validate Huawei/H3C state parser profiles.

Raw captures must not be committed here.

## Directory Layout

Use this layout:

```text
real_samples/
  huawei/
    vrp8/
      YYYYMMDD-model-os-purpose.redacted.xml
      YYYYMMDD-model-os-purpose.metadata.md
  h3c/
    comware7/
      YYYYMMDD-model-os-purpose.redacted.xml
      YYYYMMDD-model-os-purpose.metadata.md
```

Keep each XML file reduced to the smallest structure needed to prove parser behavior.

## Required Metadata

Each sample needs a sibling `.metadata.md` file:

```markdown
# Sample Metadata

- vendor:
- parser_profile:
- device_model:
- os_family:
- os_version:
- capture_date:
- capture_source: lab | staging | production-redacted
- operator:
- raw_storage_location: outside-git reference only
- redaction_notes:
- validator_command:
- validator_result:
- parser_gap:
- linked_test:
```

Use `raw_storage_location` only for an internal reference. Do not include credentials, IP addresses, serial numbers, customer names, or links that expose sensitive content.

## Redaction Requirements

Redact:

- management IP addresses and hostnames;
- usernames, passwords, keys, SNMP communities, AAA configuration, and certificates;
- serial numbers, MAC addresses, asset tags, and site names;
- customer VLAN names and interface descriptions;
- public IP addresses, routing peer addresses, customer AS numbers, and tenant names.

Preserve:

- XML namespace declarations;
- parser-relevant element names;
- VLAN ID and port-mode structure;
- interface naming style, using neutral example names;
- optional field presence or absence.

## Validator Commands

Full sample summary:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml adapter-python/tests/fixtures/state_parsers/real_samples/huawei/vrp8/sample.redacted.xml \
  --summary
```

Scoped sample summary:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample.redacted.xml \
  --summary \
  --vlan 100 \
  --interface GE1/0/1
```

Pretty observed state:

```bash
aria-underlay-state-parse \
  --vendor huawei \
  --xml sample.redacted.xml \
  --pretty
```

## Production Readiness Rule

A sample that passes validator checks is evidence, not approval. Keep parser profiles `production_ready=False` until the real sample set, tests, and parser profile have been reviewed together.
