#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import socket
import sys
from pathlib import Path
from typing import Any


RUNNER_NAME = "real-device-transaction-preflight"


def transaction_scenarios() -> list[dict[str, Any]]:
    return [
        {
            "name": "capability_and_strategy_probe",
            "goal": "Record NETCONF capabilities and recommended transaction strategy.",
            "requires_real_device": True,
            "requires_write": False,
            "evidence": [
                "raw NETCONF capabilities",
                "candidate/validate/confirmed-commit/persist-id support",
                "recommended strategy",
            ],
        },
        {
            "name": "dry_run_change_plan",
            "goal": "Prove the planned scope is limited to the approved VLAN, ACL, and interfaces.",
            "requires_real_device": True,
            "requires_write": False,
            "evidence": [
                "dry-run noop flag",
                "change sets",
                "no delete or update outside approved test resources",
            ],
        },
        {
            "name": "idempotent_apply_reuse",
            "goal": "Prove retrying the same idempotency key returns the same transaction result.",
            "requires_real_device": True,
            "requires_write": True,
            "evidence": [
                "first apply tx_id",
                "retry response tx_id",
                "single device write in adapter/audit logs",
            ],
        },
        {
            "name": "apply_verify_report",
            "goal": "Prove successful apply returns scoped verify evidence after device readback.",
            "requires_real_device": True,
            "requires_write": True,
            "evidence": [
                "apply status",
                "verify_report status",
                "readback matches requested VLAN/interface/ACL scope",
            ],
        },
        {
            "name": "commit_failure_discards_candidate",
            "goal": "Prove commit failure does not leave stale candidate data for the next retry.",
            "requires_real_device": True,
            "requires_write": True,
            "evidence": [
                "injected or controlled commit failure",
                "discard-candidate evidence",
                "next retry starts from clean candidate",
            ],
        },
        {
            "name": "rollback_failure_enters_in_doubt",
            "goal": "Prove an unknown rollback outcome blocks new writes instead of silently proceeding.",
            "requires_real_device": True,
            "requires_write": True,
            "evidence": [
                "failed rollback or adapter disconnect point",
                "journal phase",
                "new write rejected until recovery or force-resolve",
            ],
        },
        {
            "name": "startup_recovery_after_crash",
            "goal": "Prove restart recovery resolves or exposes pending journal records before new writes.",
            "requires_real_device": True,
            "requires_write": True,
            "evidence": [
                "crash point",
                "startup recovery report",
                "final journal status",
            ],
        },
        {
            "name": "force_resolve_break_glass",
            "goal": "Prove manual force-resolve clears only audited InDoubt transactions.",
            "requires_real_device": True,
            "requires_write": True,
            "evidence": [
                "operator reason",
                "before/after journal record",
                "post-resolve readback",
            ],
        },
    ]


def build_report(
    *,
    host: str,
    ssh_port: int,
    netconf_port: int,
    vendor: str,
    model: str | None,
    os_version: str | None,
    ssh_reachable: bool,
    netconf_reachable: bool,
) -> dict[str, Any]:
    blocking_reasons = []
    if not netconf_reachable:
        blocking_reasons.append("netconf port is not reachable")

    status = "blocked" if blocking_reasons else "ready_for_scoped_write_acceptance"
    warnings = []
    if not ssh_reachable:
        warnings.append(
            "ssh cli port is not reachable; NETCONF-only acceptance may still be possible, but CLI cleanup and version capture need another path"
        )

    return {
        "runner": RUNNER_NAME,
        "status": status,
        "read_only": True,
        "device": {
            "host": host,
            "ssh_port": ssh_port,
            "netconf_port": netconf_port,
            "vendor": vendor,
            "model": model,
            "os_version": os_version,
        },
        "connectivity": {
            "ssh": {"port": ssh_port, "reachable": ssh_reachable},
            "netconf": {"port": netconf_port, "reachable": netconf_reachable},
        },
        "blocking_reasons": blocking_reasons,
        "warnings": warnings,
        "transaction_scenarios": transaction_scenarios(),
        "model_ready_notes": [
            "Use the normal real-device acceptance probe only after selecting an absent VLAN/ACL and approved idle interfaces.",
            "Do not store passwords, private keys, or raw customer configuration in the report.",
        ],
    }


def check_tcp(host: str, port: int, timeout: float) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run a read-only preflight for real-device transaction acceptance. "
            "This script checks reachability and emits the transaction scenario matrix; "
            "it never accepts credentials or writes device configuration."
        )
    )
    parser.add_argument("--host", required=True, help="Switch management IP or hostname.")
    parser.add_argument("--ssh-port", type=int, default=22, help="SSH CLI port.")
    parser.add_argument("--netconf-port", type=int, default=830, help="NETCONF SSH port.")
    parser.add_argument("--vendor", default="h3c", help="Expected vendor label.")
    parser.add_argument("--model", help="Model captured from read-only CLI output.")
    parser.add_argument("--os-version", help="OS version captured from read-only CLI output.")
    parser.add_argument("--timeout", type=float, default=5.0, help="TCP timeout seconds.")
    parser.add_argument(
        "--skip-connectivity-check",
        action="store_true",
        help="Use the explicit --ssh-reachable/--netconf-reachable flags instead of opening sockets.",
    )
    parser.add_argument(
        "--ssh-reachable",
        action="store_true",
        help="Mark SSH reachable when --skip-connectivity-check is set.",
    )
    parser.add_argument(
        "--netconf-reachable",
        action="store_true",
        help="Mark NETCONF reachable when --skip-connectivity-check is set.",
    )
    parser.add_argument("--pretty", action="store_true", help="Pretty-print JSON output.")
    parser.add_argument("--json-report", type=Path, help="Optional path to write JSON report.")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)

    if args.skip_connectivity_check:
        ssh_reachable = bool(args.ssh_reachable)
        netconf_reachable = bool(args.netconf_reachable)
    else:
        ssh_reachable = check_tcp(args.host, args.ssh_port, args.timeout)
        netconf_reachable = check_tcp(args.host, args.netconf_port, args.timeout)

    report = build_report(
        host=args.host,
        ssh_port=args.ssh_port,
        netconf_port=args.netconf_port,
        vendor=args.vendor,
        model=args.model,
        os_version=args.os_version,
        ssh_reachable=ssh_reachable,
        netconf_reachable=netconf_reachable,
    )
    text = json.dumps(report, indent=2 if args.pretty else None, sort_keys=True)
    print(text)
    if args.json_report:
        args.json_report.write_text(text + "\n")

    if report["status"] == "blocked":
        for reason in report["blocking_reasons"]:
            sys.stderr.write(f"blocked: {reason}\n")
        return 1
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
