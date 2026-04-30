# Sprint 2F NETCONF State Sample Runbook Design

## Goal

Make real Huawei/H3C NETCONF running XML collection repeatable, safe to share after redaction, and directly usable with `aria-underlay-state-parse`.

## Scope

This phase adds documentation only:

- a runbook for capturing NETCONF `get-config` running XML;
- a fixture README that defines where redacted real samples live and what metadata must accompany them;
- a progress update that keeps parser production readiness blocked on real sample evidence.

The runbook does not add device credentials, does not introduce live-device test automation, and does not change parser behavior.

## Capture Flow

The preferred capture flow is:

1. capture raw running XML from a lab or field switch;
2. store the raw file outside git;
3. redact sensitive values;
4. validate the redacted file with `aria-underlay-state-parse --summary`;
5. if parser behavior differs from fixture expectations, reduce the XML into a minimal redacted fixture and add a parser test.

## Safety Rules

Raw captures must not be committed. Redaction must remove or neutralize:

- management IP addresses and hostnames;
- usernames, secrets, keys, SNMP communities, AAA configuration, and certificate material;
- serial numbers, MAC addresses, asset tags, and site identifiers;
- customer-facing interface descriptions and VLAN names;
- routing peers, public addresses, and tenant names.

Redacted samples should preserve XML structure, namespaces, field names, and parser-relevant values such as VLAN IDs and interface mode structure.

## Success Criteria

A captured sample is ready to inform parser development only when:

- it has a metadata block with vendor, model, OS version, source, capture date, and redaction notes;
- `aria-underlay-state-parse --summary` succeeds, or the failure is captured as a parser gap;
- the sample is reduced to the smallest XML needed to reproduce the parser behavior;
- a test exists before parser behavior changes.
