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
from aria_underlay_adapter.renderers.h3c import H3cRenderer


RUNNER_NAME = "offline-h3c-acceptance"
VENDOR = "h3c"


@dataclass(frozen=True)
class Scenario:
    name: str
    surface: tuple[str, ...]
    desired: dict[str, Any]
    scope: dict[str, Any]
    seed: tuple[dict[str, Any], ...] = field(default_factory=tuple)


def run_acceptance(*, backend_profile: str = "confirmed") -> dict[str, Any]:
    renderer = H3cRenderer()
    scenario_reports = []
    for scenario in _scenarios():
        try:
            scenario_reports.append(
                _run_scenario(
                    scenario,
                    renderer=renderer,
                    backend=MockNetconfBackend(backend_profile),
                )
            )
        except Exception as exc:  # pragma: no cover - exercised through reports
            scenario_reports.append(_failed_scenario_report(scenario, exc))

    passed = sum(1 for scenario in scenario_reports if scenario["status"] == "passed")
    failed = len(scenario_reports) - passed
    status = "passed" if failed == 0 else "failed"
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
            lines.append(
                "- {}: passed [{}], changed={}, xml_bytes={}".format(
                    scenario["name"],
                    surface,
                    str(scenario["changed"]).lower(),
                    scenario["xml_bytes"],
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
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    report = run_acceptance(backend_profile=args.backend_profile)
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
    backend: MockNetconfBackend,
) -> dict[str, Any]:
    for seed_desired in scenario.seed:
        _apply_desired(
            seed_desired,
            scope=scenario.scope,
            renderer=renderer,
            backend=backend,
            tx_id=f"{scenario.name}-seed",
        )

    result = _apply_desired(
        scenario.desired,
        scope=scenario.scope,
        renderer=renderer,
        backend=backend,
        tx_id=scenario.name,
    )
    return {
        "name": scenario.name,
        "status": "passed",
        "surface": list(scenario.surface),
        **result,
    }


def _apply_desired(
    desired: dict[str, Any],
    *,
    scope: dict[str, Any],
    renderer: H3cRenderer,
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

    return {
        "changed": dry_run.changed,
        "warnings": list(dry_run.warnings),
        "candidate_checksum": prepared.candidate_checksum,
        "xml_bytes": len(xml.encode("utf-8")),
        "observed_counts": {
            "vlans": len(observed.get("vlans", [])),
            "interfaces": len(observed.get("interfaces", [])),
            "acls": len(observed.get("acls", [])),
            "acl_bindings": len(observed.get("acl_bindings", [])),
        },
    }


def _failed_scenario_report(scenario: Scenario, exc: Exception) -> dict[str, Any]:
    return {
        "name": scenario.name,
        "status": "failed",
        "surface": list(scenario.surface),
        "changed": False,
        "warnings": [],
        "candidate_checksum": "",
        "xml_bytes": 0,
        "observed_counts": {
            "vlans": 0,
            "interfaces": 0,
            "acls": 0,
            "acl_bindings": 0,
        },
        "error": _error_payload(exc),
    }


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


def _namespace(value: Any) -> Any:
    if isinstance(value, dict):
        return SimpleNamespace(**{key: _namespace(inner) for key, inner in value.items()})
    if isinstance(value, list):
        return [_namespace(inner) for inner in value]
    return value


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
        "--pretty",
        action="store_true",
        help="Pretty-print JSON output.",
    )
    return parser


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
