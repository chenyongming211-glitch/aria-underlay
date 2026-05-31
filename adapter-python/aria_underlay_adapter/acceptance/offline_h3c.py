from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from pathlib import Path
from types import SimpleNamespace
from typing import Any
from xml.etree import ElementTree

from aria_underlay_adapter.backends.mock_netconf import MockNetconfBackend
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.h3c import H3C_COMWARE_CONFIG_NAMESPACE
from aria_underlay_adapter.renderers.h3c import H3cRenderer
from aria_underlay_adapter.renderers.xml import NETCONF_BASE_NAMESPACE
from aria_underlay_adapter.renderers.xml import XmlElement
from aria_underlay_adapter.renderers.xml import render_xml
from aria_underlay_adapter.state_parsers.h3c import H3cStateParser


RUNNER_NAME = "offline-h3c-acceptance"
VENDOR = "h3c"
DEFAULT_PBR_BGP_SAMPLE_DIR = (
    Path(__file__).resolve().parents[2]
    / "tests"
    / "fixtures"
    / "state_parsers"
    / "real_samples"
    / "h3c"
    / "comware7"
)


@dataclass(frozen=True)
class Scenario:
    name: str
    surface: tuple[str, ...]
    desired: dict[str, Any]
    scope: dict[str, Any]
    seed: tuple[dict[str, Any], ...] = field(default_factory=tuple)


def run_acceptance(
    *,
    backend_profile: str = "confirmed",
    pbr_bgp_sample_dir: str | Path | None = None,
) -> dict[str, Any]:
    renderer = H3cRenderer()
    parser = H3cStateParser(model_hint="S5560-54C-EI")
    scenario_reports = []
    for scenario in _scenarios():
        try:
            scenario_reports.append(
                _run_scenario(
                    scenario,
                    renderer=renderer,
                    parser=parser,
                    backend=MockNetconfBackend(backend_profile),
                )
            )
        except Exception as exc:  # pragma: no cover - exercised through reports
            scenario_reports.append(_failed_scenario_report(scenario, exc))

    read_only_audits = [_run_pbr_bgp_read_only_audit(parser)]
    real_sample_audits = _run_pbr_bgp_real_sample_audits(
        parser,
        sample_dir=Path(pbr_bgp_sample_dir)
        if pbr_bgp_sample_dir is not None
        else DEFAULT_PBR_BGP_SAMPLE_DIR,
    )
    passed = sum(1 for scenario in scenario_reports if scenario["status"] == "passed")
    failed = len(scenario_reports) - passed
    audit_failed = sum(1 for audit in read_only_audits if audit["status"] != "passed")
    real_sample_audit_failed = sum(
        1 for audit in real_sample_audits if audit["status"] != "passed"
    )
    status = (
        "passed"
        if failed == 0 and audit_failed == 0 and real_sample_audit_failed == 0
        else "failed"
    )
    return {
        "runner": RUNNER_NAME,
        "vendor": VENDOR,
        "profile_name": renderer.profile.profile_name,
        "production_ready": renderer.production_ready,
        "backend_profile": backend_profile,
        "status": status,
        "scenario_count": len(scenario_reports),
        "passed": passed,
        "failed": failed,
        "scenarios": scenario_reports,
        "read_only_audit_count": len(read_only_audits),
        "read_only_audit_failed": audit_failed,
        "read_only_audits": read_only_audits,
        "real_sample_audit_count": len(real_sample_audits),
        "real_sample_audit_failed": real_sample_audit_failed,
        "real_sample_audits": real_sample_audits,
    }


def format_summary(report: dict[str, Any]) -> str:
    lines = [
        "Offline H3C acceptance: {} ({}/{})".format(
            report["status"],
            report["passed"],
            report["scenario_count"],
        )
    ]
    for scenario in report["scenarios"]:
        surface = ", ".join(scenario["surface"])
        if scenario["status"] == "passed":
            change_plan = scenario["change_plan"]
            lines.append(
                "- {}: passed [{}], changed={}, xml_bytes={}, "
                "readback_xml_bytes={}, blast_radius={}, parser_loop=true".format(
                    scenario["name"],
                    surface,
                    str(scenario["changed"]).lower(),
                    scenario["xml_bytes"],
                    scenario["readback_xml_bytes"],
                    change_plan["blast_radius"],
                )
            )
        else:
            error = scenario.get("error", {})
            lines.append(
                "- {}: failed [{}], {}: {}".format(
                    scenario["name"],
                    surface,
                    error.get("code", "ERROR"),
                    error.get("message", ""),
                )
            )
    for audit in report.get("read_only_audits", []):
        if audit["status"] == "passed":
            touched_scope = audit.get("touched_scope", {})
            lines.append(
                "- {}: passed [{}], changed={}, write_decision={}, "
                "blast_radius={}, vrfs={}, bgp_neighbors={}, "
                "route_policies={}, pbr_policies={}, acl_refs={}, interfaces={}".format(
                    audit["name"],
                    ", ".join(audit["surface"]),
                    str(audit["changed"]).lower(),
                    audit["write_decision"],
                    audit["blast_radius"],
                    _format_summary_values(touched_scope.get("affected_vrfs", [])),
                    _format_summary_values(touched_scope.get("bgp_neighbors", [])),
                    _format_summary_values(touched_scope.get("route_policy_refs", [])),
                    _format_summary_values(touched_scope.get("pbr_policy_refs", [])),
                    _format_summary_values(touched_scope.get("acl_refs", [])),
                    _format_summary_values(touched_scope.get("interfaces", [])),
                )
            )
        else:
            error = audit.get("error", {})
            lines.append(
                "- {}: failed [{}], {}: {}".format(
                    audit["name"],
                    ", ".join(audit["surface"]),
                    error.get("code", "ERROR"),
                    error.get("message", ""),
                )
            )
    for audit in report.get("real_sample_audits", []):
        if audit["status"] == "passed":
            touched_scope = audit.get("touched_scope", {})
            lines.append(
                "- {}: passed [{}], sample={}, changed={}, "
                "write_decision={}, blast_radius={}, vrfs={}, "
                "bgp_neighbors={}, route_policies={}, pbr_policies={}, "
                "acl_refs={}, interfaces={}".format(
                    audit["name"],
                    ", ".join(audit["surface"]),
                    audit["sample_path"],
                    str(audit["changed"]).lower(),
                    audit["write_decision"],
                    audit["blast_radius"],
                    _format_summary_values(touched_scope.get("affected_vrfs", [])),
                    _format_summary_values(touched_scope.get("bgp_neighbors", [])),
                    _format_summary_values(touched_scope.get("route_policy_refs", [])),
                    _format_summary_values(touched_scope.get("pbr_policy_refs", [])),
                    _format_summary_values(touched_scope.get("acl_refs", [])),
                    _format_summary_values(touched_scope.get("interfaces", [])),
                )
            )
        else:
            error = audit.get("error", {})
            lines.append(
                "- {}: failed [{}], sample={}, {}: {}".format(
                    audit["name"],
                    ", ".join(audit["surface"]),
                    audit.get("sample_path", ""),
                    error.get("code", "ERROR"),
                    error.get("message", ""),
                )
            )
    return "\n".join(lines) + "\n"


def _format_summary_values(values: list[Any]) -> str:
    return ",".join(str(value) for value in values) if values else "-"


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    report = run_acceptance(
        backend_profile=args.backend_profile,
        pbr_bgp_sample_dir=args.pbr_bgp_sample_dir,
    )
    json_output = _to_json(report, pretty=args.pretty)
    summary = format_summary(report)

    print(json_output)
    sys.stderr.write(summary)

    if args.json_report:
        args.json_report.write_text(json_output + "\n")
    if args.summary:
        args.summary.write_text(summary)

    return 0 if report["status"] == "passed" else 1


def _run_scenario(
    scenario: Scenario,
    *,
    renderer: H3cRenderer,
    parser: H3cStateParser,
    backend: MockNetconfBackend,
) -> dict[str, Any]:
    for seed_desired in scenario.seed:
        _apply_desired(
            seed_desired,
            scope=scenario.scope,
            renderer=renderer,
            parser=parser,
            backend=backend,
            tx_id=f"{scenario.name}-seed",
        )

    result = _apply_desired(
        scenario.desired,
        scope=scenario.scope,
        renderer=renderer,
        parser=parser,
        backend=backend,
        tx_id=scenario.name,
    )
    return {
        "name": scenario.name,
        "status": "passed",
        "surface": list(scenario.surface),
        "change_plan": _change_plan_report(scenario),
        **result,
    }


def _apply_desired(
    desired: dict[str, Any],
    *,
    scope: dict[str, Any],
    renderer: H3cRenderer,
    parser: H3cStateParser,
    backend: MockNetconfBackend,
    tx_id: str,
) -> dict[str, Any]:
    desired_state = _namespace(desired)
    scope_state = _namespace(scope)
    xml = renderer.render_edit_config(desired_state)
    ElementTree.fromstring(xml)

    dry_run = backend.dry_run_candidate(desired_state)
    prepared = backend.prepare_candidate(desired_state)
    backend.commit_candidate(
        strategy="confirmed_commit",
        tx_id=tx_id,
        prepared_candidate_checksum=prepared.candidate_checksum,
    )
    backend.final_confirm(tx_id=tx_id)
    backend.verify_running(desired_state, scope=scope_state)
    observed = backend.get_current_state(scope=scope_state)
    expected_parsed_state = _h3c_comparable_state(observed)
    readback_xml = _h3c_running_xml(expected_parsed_state)
    ElementTree.fromstring(readback_xml)
    parsed_state = _h3c_comparable_state(
        parser.parse_running(readback_xml, scope=scope_state)
    )
    if parsed_state != expected_parsed_state:
        raise AdapterError(
            code="H3C_PARSER_LOOP_MISMATCH",
            message="offline H3C parser-in-the-loop verification failed",
            normalized_error="h3c parser loop mismatch",
            raw_error_summary=(
                f"expected={expected_parsed_state!r}, parsed={parsed_state!r}"
            ),
            retryable=False,
        )

    return {
        "stages": ["render", "apply", "parse", "verify"],
        "changed": dry_run.changed,
        "warnings": list(dry_run.warnings),
        "candidate_checksum": prepared.candidate_checksum,
        "xml_bytes": len(xml.encode("utf-8")),
        "readback_xml_bytes": len(readback_xml.encode("utf-8")),
        "parser_profile_name": parser.profile.profile_name,
        "observed_counts": _state_counts(expected_parsed_state),
        "parsed_counts": _state_counts(parsed_state),
    }


def _failed_scenario_report(scenario: Scenario, exc: Exception) -> dict[str, Any]:
    return {
        "name": scenario.name,
        "status": "failed",
        "surface": list(scenario.surface),
        "change_plan": _change_plan_report(scenario),
        "changed": False,
        "warnings": [],
        "candidate_checksum": "",
        "xml_bytes": 0,
        "readback_xml_bytes": 0,
        "parser_profile_name": "comware7-state-real",
        "stages": ["render", "apply", "parse", "verify"],
        "observed_counts": {
            "vlans": 0,
            "interfaces": 0,
            "acls": 0,
            "acl_bindings": 0,
        },
        "parsed_counts": {
            "vlans": 0,
            "interfaces": 0,
            "acls": 0,
            "acl_bindings": 0,
        },
        "error": _error_payload(exc),
    }


def _run_pbr_bgp_read_only_audit(parser: H3cStateParser) -> dict[str, Any]:
    try:
        parsed = parser.parse_running(_h3c_high_risk_audit_xml())
        audit = parsed["high_risk_audit"]
    except Exception as exc:  # pragma: no cover - exercised through reports
        return {
            "name": "pbr_bgp_high_risk_read_only",
            "status": "failed",
            "surface": ["pbr", "bgp"],
            "stages": ["parse", "audit"],
            "changed": False,
            "write_decision": "rejected",
            "features_present": [],
            "blast_radius": "routing_control_plane",
            "unsupported_paths": [
                "bgp: parser audit failed",
                "pbr: parser audit failed",
            ],
            "warnings": [],
            "pbr": {},
            "bgp": {},
            "touched_scope": _empty_high_risk_touched_scope(),
            "error": _error_payload(exc),
        }

    return {
        "name": "pbr_bgp_high_risk_read_only",
        "status": "passed",
        "surface": ["pbr", "bgp"],
        "stages": ["parse", "audit"],
        "changed": False,
        "write_decision": audit["write_decision"],
        "features_present": audit["features_present"],
        "blast_radius": "routing_control_plane",
        "unsupported_paths": [
            "bgp: no path-level write evidence",
            "pbr: no path-level write evidence",
        ],
        "touched_scope": audit["touched_scope"],
        "pbr": audit["pbr"],
        "bgp": audit["bgp"],
        "warnings": audit["warnings"],
    }


def _run_pbr_bgp_real_sample_audits(
    parser: H3cStateParser,
    *,
    sample_dir: Path,
) -> list[dict[str, Any]]:
    if not sample_dir.exists():
        return []
    if not sample_dir.is_dir():
        return [
            _failed_real_sample_audit(
                sample_dir,
                ValueError(f"PBR/BGP sample path is not a directory: {sample_dir}"),
            )
        ]

    audits = []
    for sample in sorted(sample_dir.rglob("*.xml")):
        try:
            audits.append(_run_pbr_bgp_real_sample_audit(parser, sample))
        except Exception as exc:  # pragma: no cover - exercised through reports
            audits.append(_failed_real_sample_audit(sample, exc))
    return audits


def _run_pbr_bgp_real_sample_audit(
    parser: H3cStateParser,
    sample: Path,
) -> dict[str, Any]:
    parsed = parser.parse_running(sample.read_text())
    audit = parsed.get("high_risk_audit")
    if audit is None:
        audit = _empty_high_risk_audit()

    unsupported_paths = []
    if "bgp" in audit["features_present"]:
        unsupported_paths.append("bgp: no path-level write evidence")
    if "pbr" in audit["features_present"]:
        unsupported_paths.append("pbr: no path-level write evidence")

    return {
        "name": f"real_sample:{sample.name}",
        "status": "passed",
        "sample_source": "real_sample",
        "sample_path": str(sample),
        "surface": ["pbr", "bgp"],
        "stages": ["parse", "audit"],
        "changed": False,
        "write_decision": audit["write_decision"],
        "features_present": audit["features_present"],
        "blast_radius": "routing_control_plane",
        "unsupported_paths": unsupported_paths,
        "touched_scope": audit["touched_scope"],
        "pbr": audit["pbr"],
        "bgp": audit["bgp"],
        "warnings": audit["warnings"],
    }


def _failed_real_sample_audit(sample: Path, exc: Exception) -> dict[str, Any]:
    return {
        "name": f"real_sample:{sample.name}",
        "status": "failed",
        "sample_source": "real_sample",
        "sample_path": str(sample),
        "surface": ["pbr", "bgp"],
        "stages": ["parse", "audit"],
        "changed": False,
        "write_decision": "rejected",
        "features_present": [],
        "blast_radius": "routing_control_plane",
        "unsupported_paths": [
            "bgp: parser audit failed",
            "pbr: parser audit failed",
        ],
        "warnings": [],
        "pbr": {},
        "bgp": {},
        "touched_scope": _empty_high_risk_touched_scope(),
        "error": _error_payload(exc),
    }


def _empty_high_risk_audit() -> dict[str, Any]:
    return {
        "features_present": [],
        "write_decision": "no_high_risk_features",
        "touched_scope": _empty_high_risk_touched_scope(),
        "pbr": {
            "present": False,
            "blast_radius": "policy_reference",
            "policies": [],
            "acl_references": [],
            "interfaces": [],
            "raw_paths": [],
        },
        "bgp": {
            "present": False,
            "blast_radius": "routing_control_plane",
            "as_numbers": [],
            "vrfs": [],
            "neighbors": [],
            "policy_references": [],
            "raw_paths": [],
        },
        "warnings": ["no PBR/BGP config detected in sample"],
    }


def _empty_high_risk_touched_scope() -> dict[str, list[Any]]:
    return {
        "affected_vrfs": [],
        "bgp_as_numbers": [],
        "bgp_neighbors": [],
        "route_policy_refs": [],
        "pbr_policy_refs": [],
        "acl_refs": [],
        "interfaces": [],
        "raw_paths": [],
    }


def _change_plan_report(scenario: Scenario) -> dict[str, Any]:
    surface = set(scenario.surface)
    is_policy = any("acl" in item for item in surface)
    has_delete = any(item.startswith("delete_") for item in surface)

    stages: list[str] = []
    if has_delete:
        if "delete_acl_binding" in surface:
            stages.append("unbind_references")
        stages.append("delete_base_objects")
    else:
        if {"vlan_create", "ipv4_advanced_acl", "ipv4_basic_acl"} & surface:
            stages.append("create_base_objects")
        if {
            "vlan_description",
            "access_interface",
            "interface_description",
            "trunk_interface",
        } & surface:
            stages.append("update_base_objects")
        if "acl_interface_binding" in surface:
            stages.append("bind_references")

    if not stages:
        stages.append("update_base_objects")

    dependency_edges: list[dict[str, str]] = []
    if "acl_interface_binding" in surface:
        dependency_edges.append(
            {
                "from": "acl-binding interface inbound",
                "to": "acl object",
            }
        )
    if "delete_acl" in surface:
        dependency_edges.append(
            {
                "from": "acl delete",
                "to": "all acl bindings unbound",
            }
        )

    rollback_order = _rollback_order_for_surface(surface)
    return {
        "stages": stages,
        "dependency_edges": dependency_edges,
        "blast_radius": "policy_reference" if is_policy else "local_interface_or_vlan",
        "rollback_order": rollback_order,
    }


def _rollback_order_for_surface(surface: set[str]) -> list[str]:
    rollback_order: list[str] = []
    if "acl_interface_binding" in surface:
        rollback_order.append("remove acl interface binding")
    if "ipv4_advanced_acl" in surface or "ipv4_basic_acl" in surface:
        rollback_order.append("restore or delete acl")
    if "trunk_interface" in surface or "access_interface" in surface:
        rollback_order.append("restore interface mode")
    if "vlan_create" in surface or "delete_vlan" in surface:
        rollback_order.append("restore vlan state")
    if "delete_acl_binding" in surface:
        rollback_order.append("restore acl interface binding")
    if "delete_acl" in surface:
        rollback_order.append("restore acl")
    if not rollback_order:
        rollback_order.append("restore touched resources")
    return rollback_order


def _error_payload(exc: Exception) -> dict[str, Any]:
    if isinstance(exc, AdapterError):
        return {
            "code": exc.code,
            "message": exc.message,
            "normalized_error": exc.normalized_error,
            "raw_error_summary": exc.raw_error_summary,
            "retryable": exc.retryable,
        }
    return {
        "code": type(exc).__name__,
        "message": str(exc),
        "normalized_error": "offline h3c acceptance failed",
        "raw_error_summary": str(exc),
        "retryable": False,
    }


def _scenarios() -> tuple[Scenario, ...]:
    return (
        Scenario(
            name="vlan_access_description",
            surface=(
                "vlan_create",
                "vlan_description",
                "access_interface",
                "interface_description",
            ),
            desired={
                "vlans": [
                    {
                        "vlan_id": 144,
                        "name": "aria-acceptance",
                        "description": "offline acceptance vlan",
                    }
                ],
                "interfaces": [
                    {
                        "name": "GigabitEthernet1/0/13",
                        "admin_state": "up",
                        "description": "offline acceptance access",
                        "mode": {
                            "kind": "access",
                            "access_vlan": 144,
                            "native_vlan": None,
                            "allowed_vlans": [],
                        },
                    }
                ],
            },
            scope={
                "full": False,
                "vlan_ids": [144],
                "interface_names": ["GigabitEthernet1/0/13"],
                "acl_ids": [],
            },
        ),
        Scenario(
            name="trunk_allowed_vlans",
            surface=("trunk_interface",),
            desired={
                "vlans": [],
                "interfaces": [
                    {
                        "name": "Ten-GigabitEthernet1/0/21",
                        "admin_state": "up",
                        "description": None,
                        "mode": {
                            "kind": "trunk",
                            "access_vlan": None,
                            "native_vlan": None,
                            "allowed_vlans": [144, 145, 146],
                        },
                    }
                ],
            },
            scope={
                "full": False,
                "vlan_ids": [],
                "interface_names": ["Ten-GigabitEthernet1/0/21"],
                "acl_ids": [],
            },
        ),
        Scenario(
            name="ipv4_basic_acl_rules",
            surface=("ipv4_basic_acl", "acl_rule_description"),
            desired={
                "vlans": [],
                "interfaces": [],
                "acls": [
                    {
                        "acl_id": 2001,
                        "kind": "basic_ipv4",
                        "name": None,
                        "description": "offline acceptance basic acl",
                        "rules": [
                            {
                                "sequence": 5,
                                "action": "permit",
                                "protocol": "ip",
                                "source": {
                                    "address": "192.0.2.0",
                                    "wildcard": "0.0.0.255",
                                },
                                "destination": None,
                                "source_port_eq": None,
                                "destination_port_eq": None,
                                "description": "allow offline source",
                            }
                        ],
                    }
                ],
            },
            scope={
                "full": False,
                "vlan_ids": [],
                "interface_names": [],
                "acl_ids": [2001],
            },
        ),
        Scenario(
            name="ipv4_acl_rules",
            surface=("ipv4_advanced_acl", "acl_rule_description"),
            desired={
                "vlans": [],
                "interfaces": [],
                "acls": [
                    {
                        "acl_id": 3999,
                        "name": None,
                        "description": "offline acceptance acl",
                        "rules": [
                            {
                                "sequence": 10,
                                "action": "permit",
                                "protocol": "ip",
                                "source": {
                                    "address": "192.0.2.1",
                                    "wildcard": "0.0.0.0",
                                },
                                "destination": {
                                    "address": "198.51.100.0",
                                    "wildcard": "0.0.0.255",
                                },
                                "source_port_eq": None,
                                "destination_port_eq": None,
                                "description": "allow offline test flow",
                            },
                            {
                                "sequence": 20,
                                "action": "deny",
                                "protocol": "tcp",
                                "source": {
                                    "address": "192.0.2.0",
                                    "wildcard": "0.0.0.255",
                                },
                                "destination": {
                                    "address": "198.51.100.10",
                                    "wildcard": "0.0.0.0",
                                },
                                "source_port_eq": None,
                                "destination_port_eq": 443,
                                "description": None,
                            },
                        ],
                    }
                ],
            },
            scope={
                "full": False,
                "vlan_ids": [],
                "interface_names": [],
                "acl_ids": [3999],
            },
        ),
        Scenario(
            name="acl_interface_binding",
            surface=("ipv4_advanced_acl", "acl_interface_binding"),
            desired={
                "vlans": [],
                "interfaces": [],
                "acls": [
                    {
                        "acl_id": 3997,
                        "name": None,
                        "description": "offline acceptance binding acl",
                        "rules": [],
                    }
                ],
                "acl_bindings": [
                    {
                        "interface_name": "GigabitEthernet1/0/13",
                        "direction": "inbound",
                        "acl_id": 3997,
                    }
                ],
            },
            scope={
                "full": False,
                "vlan_ids": [],
                "interface_names": [],
                "acl_ids": [3997],
            },
        ),
        Scenario(
            name="explicit_delete_cleanup",
            surface=("delete_vlan", "delete_acl", "delete_acl_binding"),
            seed=(
                {
                    "vlans": [
                        {
                            "vlan_id": 144,
                            "name": "aria-acceptance",
                            "description": "offline acceptance vlan",
                        }
                    ],
                    "interfaces": [],
                    "acls": [
                        {
                            "acl_id": 3998,
                            "name": None,
                            "description": "offline acceptance cleanup acl",
                            "rules": [
                                {
                                    "sequence": 10,
                                    "action": "permit",
                                    "protocol": "ip",
                                    "source": None,
                                    "destination": None,
                                    "source_port_eq": None,
                                    "destination_port_eq": None,
                                    "description": "temporary cleanup rule",
                                }
                            ],
                        }
                    ],
                    "acl_bindings": [
                        {
                            "interface_name": "GigabitEthernet1/0/13",
                            "direction": "inbound",
                            "acl_id": 3998,
                        }
                    ],
                },
            ),
            desired={
                "vlans": [],
                "interfaces": [],
                "acls": [],
                "acl_bindings": [],
                "delete_vlan_ids": [144],
                "delete_acl_ids": [3998],
                "delete_acl_bindings": [
                    {
                        "interface_name": "GigabitEthernet1/0/13",
                        "direction": "inbound",
                        "acl_id": 3998,
                    }
                ],
            },
            scope={
                "full": False,
                "vlan_ids": [144],
                "interface_names": [],
                "acl_ids": [3998],
            },
        ),
    )


def _h3c_high_risk_audit_xml() -> str:
    return """
    <data xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
      <top xmlns="http://www.h3c.com/netconf/config:1.0">
        <PBR>
          <Policies>
            <Policy>
              <PolicyName>pbr-tenant-a</PolicyName>
              <AclNumber>3999</AclNumber>
              <ApplyInterface>GigabitEthernet1/0/13</ApplyInterface>
            </Policy>
          </Policies>
        </PBR>
        <BGP>
          <Instances>
            <Instance>
              <ASNumber>65001</ASNumber>
              <VRF>tenant-a</VRF>
              <Peers>
                <Peer>
                  <PeerAddress>192.0.2.1</PeerAddress>
                  <ImportPolicy>rp-in</ImportPolicy>
                </Peer>
              </Peers>
            </Instance>
          </Instances>
        </BGP>
      </top>
    </data>
    """


def _namespace(value: Any) -> Any:
    if isinstance(value, dict):
        return SimpleNamespace(**{key: _namespace(inner) for key, inner in value.items()})
    if isinstance(value, list):
        return [_namespace(inner) for inner in value]
    return value


def _h3c_running_xml(state: dict[str, Any]) -> str:
    top_children = []
    ifmgr = _ifmgr_node(state.get("interfaces", []))
    if ifmgr is not None:
        top_children.append(ifmgr)
    top_children.append(_vlan_node(state))
    top_children.append(_acl_node(state))
    return render_xml(
        XmlElement(
            "data",
            namespace=NETCONF_BASE_NAMESPACE,
            children=[
                XmlElement(
                    "top",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=top_children,
                )
            ],
        )
    )


def _ifmgr_node(interfaces: list[dict[str, Any]]) -> XmlElement | None:
    interface_nodes = []
    for interface in interfaces:
        description = interface.get("description")
        if not description:
            continue
        interface_nodes.append(
            XmlElement(
                "Interface",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=[
                    XmlElement(
                        "IfIndex",
                        namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                        children=[str(_interface_ifindex(interface["name"]))],
                    ),
                    XmlElement(
                        "Description",
                        namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                        children=[description],
                    ),
                ],
            )
        )
    if not interface_nodes:
        return None
    return XmlElement(
        "Ifmgr",
        namespace=H3C_COMWARE_CONFIG_NAMESPACE,
        children=[
            XmlElement(
                "Interfaces",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=interface_nodes,
            )
        ],
    )


def _vlan_node(state: dict[str, Any]) -> XmlElement:
    vlan_children = []
    vlan_nodes = [
        XmlElement(
            "VLANID",
            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
            children=[
                XmlElement(
                    "ID",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[str(vlan["vlan_id"])],
                ),
                *_optional_xml_text("Name", vlan.get("name")),
                *_optional_xml_text("Description", vlan.get("description")),
            ],
        )
        for vlan in state.get("vlans", [])
    ]
    if vlan_nodes:
        vlan_children.append(
            XmlElement(
                "VLANs",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=vlan_nodes,
            )
        )

    access_nodes = []
    trunk_nodes = []
    for interface in state.get("interfaces", []):
        mode = interface["mode"]
        if mode["kind"] == "access":
            access_nodes.append(
                XmlElement(
                    "Interface",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[
                        XmlElement(
                            "IfIndex",
                            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                            children=[str(_interface_ifindex(interface["name"]))],
                        ),
                        XmlElement(
                            "PVID",
                            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                            children=[str(mode["access_vlan"])],
                        ),
                    ],
                )
            )
        elif mode["kind"] == "trunk":
            trunk_nodes.append(
                XmlElement(
                    "Interface",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[
                        XmlElement(
                            "IfIndex",
                            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                            children=[str(_interface_ifindex(interface["name"]))],
                        ),
                        XmlElement(
                            "PermitVlanList",
                            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                            children=[_format_vlan_ranges(mode.get("allowed_vlans", []))],
                        ),
                    ],
                )
            )
    if access_nodes:
        vlan_children.append(
            XmlElement(
                "AccessInterfaces",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=access_nodes,
            )
        )
    if trunk_nodes:
        vlan_children.append(
            XmlElement(
                "TrunkInterfaces",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=trunk_nodes,
            )
        )
    return XmlElement("VLAN", namespace=H3C_COMWARE_CONFIG_NAMESPACE, children=vlan_children)


def _acl_node(state: dict[str, Any]) -> XmlElement:
    acl_children = []
    group_nodes = []
    advanced_rule_nodes = []
    basic_rule_nodes = []
    for acl in state.get("acls", []):
        group_nodes.append(
            XmlElement(
                "Group",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=[
                    XmlElement(
                        "GroupType",
                        namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                        children=["1"],
                    ),
                    XmlElement(
                        "GroupID",
                        namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                        children=[str(acl["acl_id"])],
                    ),
                    *_optional_xml_text("Description", acl.get("description")),
                ],
            )
        )
        for rule in acl.get("rules", []):
            if acl.get("kind") == "basic_ipv4":
                basic_rule_nodes.append(_acl_rule_node(acl["acl_id"], rule, kind="basic_ipv4"))
            else:
                advanced_rule_nodes.append(
                    _acl_rule_node(acl["acl_id"], rule, kind="advanced_ipv4")
                )
    if group_nodes:
        acl_children.append(
            XmlElement(
                "Groups",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=group_nodes,
            )
        )
    if basic_rule_nodes:
        acl_children.append(
            XmlElement(
                "IPv4BasicRules",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=basic_rule_nodes,
            )
        )
    if advanced_rule_nodes:
        acl_children.append(
            XmlElement(
                "IPv4AdvanceRules",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=advanced_rule_nodes,
            )
        )

    binding_nodes = [
        XmlElement(
            "Pfilter",
            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
            children=[
                XmlElement(
                    "AppObjType",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=["1"],
                ),
                XmlElement(
                    "AppObjIndex",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[str(_interface_ifindex(binding["interface_name"]))],
                ),
                XmlElement(
                    "AppDirection",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[str(_acl_direction_code(binding["direction"]))],
                ),
                XmlElement(
                    "AppAclType",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=["1"],
                ),
                XmlElement(
                    "AppAclGroup",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[str(binding["acl_id"])],
                ),
            ],
        )
        for binding in state.get("acl_bindings", [])
    ]
    if binding_nodes:
        acl_children.append(
            XmlElement(
                "PfilterApply",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=binding_nodes,
            )
        )
    return XmlElement("ACL", namespace=H3C_COMWARE_CONFIG_NAMESPACE, children=acl_children)


def _acl_rule_node(acl_id: int, rule: dict[str, Any], *, kind: str) -> XmlElement:
    children = [
        XmlElement("GroupID", namespace=H3C_COMWARE_CONFIG_NAMESPACE, children=[str(acl_id)]),
        XmlElement(
            "RuleID",
            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
            children=[str(rule["sequence"])],
        ),
        XmlElement(
            "Action",
            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
            children=[str(_acl_action_code(rule["action"]))],
        ),
        *_optional_xml_text("Description", rule.get("description")),
    ]
    if kind == "advanced_ipv4":
        children.append(
            XmlElement(
                "ProtocolType",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=[str(_acl_protocol_code(rule["protocol"]))],
            )
        )
    if rule.get("source") is not None:
        children.extend(_acl_endpoint_nodes("Src", rule["source"]))
    if kind == "advanced_ipv4" and rule.get("destination") is not None:
        children.extend(_acl_endpoint_nodes("Dst", rule["destination"]))
    if kind == "advanced_ipv4" and rule.get("source_port_eq") is not None:
        children.append(_acl_port_node("Src", rule["source_port_eq"]))
    if kind == "advanced_ipv4" and rule.get("destination_port_eq") is not None:
        children.append(_acl_port_node("Dst", rule["destination_port_eq"]))
    return XmlElement("Rule", namespace=H3C_COMWARE_CONFIG_NAMESPACE, children=children)


def _acl_endpoint_nodes(prefix: str, endpoint: dict[str, str]) -> list[XmlElement]:
    return [
        XmlElement(f"{prefix}Any", namespace=H3C_COMWARE_CONFIG_NAMESPACE, children=["false"]),
        XmlElement(
            f"{prefix}IPv4",
            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
            children=[
                XmlElement(
                    f"{prefix}IPv4Addr",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[endpoint["address"]],
                ),
                XmlElement(
                    f"{prefix}IPv4Wildcard",
                    namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                    children=[endpoint["wildcard"]],
                ),
            ],
        ),
    ]


def _acl_port_node(prefix: str, value: int) -> XmlElement:
    return XmlElement(
        f"{prefix}Port",
        namespace=H3C_COMWARE_CONFIG_NAMESPACE,
        children=[
            XmlElement(f"{prefix}PortOp", namespace=H3C_COMWARE_CONFIG_NAMESPACE, children=["2"]),
            XmlElement(
                f"{prefix}PortValue1",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=[str(value)],
            ),
            XmlElement(
                f"{prefix}PortValue2",
                namespace=H3C_COMWARE_CONFIG_NAMESPACE,
                children=["65536"],
            ),
        ],
    )


def _optional_xml_text(name: str, value: Any) -> list[XmlElement]:
    if value is None or value == "":
        return []
    return [
        XmlElement(
            name,
            namespace=H3C_COMWARE_CONFIG_NAMESPACE,
            children=[str(value)],
        )
    ]


def _h3c_comparable_state(state: dict[str, Any]) -> dict[str, Any]:
    return {
        "vlans": sorted(
            [
                {
                    "vlan_id": int(vlan["vlan_id"]),
                    "name": vlan.get("name"),
                    "description": vlan.get("description"),
                }
                for vlan in state.get("vlans", [])
            ],
            key=lambda vlan: vlan["vlan_id"],
        ),
        "interfaces": sorted(
            [
                {
                    "name": interface["name"],
                    "admin_state": None,
                    "description": interface.get("description"),
                    "mode": _comparable_mode(interface["mode"]),
                }
                for interface in state.get("interfaces", [])
            ],
            key=lambda interface: interface["name"],
        ),
        "acls": sorted(
            [
                _without_none_fields({
                    "acl_id": int(acl["acl_id"]),
                    "kind": acl.get("kind"),
                    "name": acl.get("name"),
                    "description": acl.get("description"),
                    "rules": sorted(
                        [_comparable_acl_rule(rule) for rule in acl.get("rules", [])],
                        key=lambda rule: rule["sequence"],
                    ),
                })
                for acl in state.get("acls", [])
            ],
            key=lambda acl: acl["acl_id"],
        ),
        "acl_bindings": sorted(
            [
                {
                    "interface_name": binding["interface_name"],
                    "direction": binding["direction"],
                    "acl_id": int(binding["acl_id"]),
                }
                for binding in state.get("acl_bindings", [])
            ],
            key=lambda binding: (binding["interface_name"], binding["direction"]),
        ),
}


def _without_none_fields(value: dict[str, Any]) -> dict[str, Any]:
    return {key: item for key, item in value.items() if item is not None}


def _comparable_mode(mode: dict[str, Any]) -> dict[str, Any]:
    if mode["kind"] == "access":
        return {
            "kind": "access",
            "access_vlan": int(mode["access_vlan"]),
            "native_vlan": None,
            "allowed_vlans": [],
        }
    return {
        "kind": "trunk",
        "access_vlan": None,
        "native_vlan": None,
        "allowed_vlans": sorted(int(vlan_id) for vlan_id in mode.get("allowed_vlans", [])),
    }


def _comparable_acl_rule(rule: dict[str, Any]) -> dict[str, Any]:
    return {
        "sequence": int(rule["sequence"]),
        "action": rule["action"],
        "protocol": rule["protocol"],
        "source": rule.get("source"),
        "destination": rule.get("destination"),
        "source_port_eq": rule.get("source_port_eq"),
        "destination_port_eq": rule.get("destination_port_eq"),
        "description": rule.get("description"),
    }


def _state_counts(state: dict[str, Any]) -> dict[str, int]:
    return {
        "vlans": len(state.get("vlans", [])),
        "interfaces": len(state.get("interfaces", [])),
        "acls": len(state.get("acls", [])),
        "acl_bindings": len(state.get("acl_bindings", [])),
    }


def _interface_ifindex(name: str) -> int:
    text = str(name).strip()
    try:
        return int(text.rsplit("/", 1)[1].split(".", 1)[0])
    except (IndexError, ValueError) as exc:
        raise ValueError(f"unsupported H3C interface name: {name}") from exc


def _format_vlan_ranges(vlan_ids: list[int]) -> str:
    values = sorted(int(vlan_id) for vlan_id in vlan_ids)
    if not values:
        return ""
    ranges = []
    start = values[0]
    previous = values[0]
    for vlan_id in values[1:]:
        if vlan_id == previous + 1:
            previous = vlan_id
            continue
        ranges.append(_format_vlan_range(start, previous))
        start = vlan_id
        previous = vlan_id
    ranges.append(_format_vlan_range(start, previous))
    return ",".join(ranges)


def _format_vlan_range(start: int, end: int) -> str:
    return str(start) if start == end else f"{start}-{end}"


def _acl_action_code(action: str) -> int:
    return {"deny": 1, "permit": 2}[action]


def _acl_protocol_code(protocol: str) -> int:
    return {"icmp": 1, "tcp": 6, "udp": 17, "ip": 256}[protocol]


def _acl_direction_code(direction: str) -> int:
    return {"inbound": 1, "outbound": 2}[direction]


def _to_json(payload: dict[str, Any], *, pretty: bool = False) -> str:
    if pretty:
        return json.dumps(payload, indent=2, sort_keys=True)
    return json.dumps(payload, sort_keys=True)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="aria-underlay-h3c-offline-acceptance",
        description="Run offline H3C Comware command-surface acceptance against the mock NETCONF backend.",
    )
    parser.add_argument(
        "--backend-profile",
        default="confirmed",
        help="Mock NETCONF backend profile to use. Defaults to confirmed.",
    )
    parser.add_argument(
        "--json-report",
        type=Path,
        help="Optional path for the machine-readable JSON report.",
    )
    parser.add_argument(
        "--summary",
        type=Path,
        help="Optional path for the human-readable summary.",
    )
    parser.add_argument(
        "--pbr-bgp-sample-dir",
        type=Path,
        default=None,
        help=(
            "Optional directory of redacted H3C running XML samples for "
            "PBR/BGP read-only calibration. Missing directories are ignored."
        ),
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Pretty-print JSON output.",
    )
    return parser


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
