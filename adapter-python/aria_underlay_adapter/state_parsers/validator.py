from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.state_parsers.registry import state_parser_for_vendor


def main(argv=None) -> int:
    args = _parser().parse_args(argv)
    xml = args.xml.read()
    scope = _Scope(
        full=args.full or (not args.vlan and not args.interface),
        vlan_ids=args.vlan,
        interface_names=args.interface,
    )
    try:
        parser = state_parser_for_vendor(args.vendor, allow_fixture_verified=True)
        state = parser.parse_running(xml, scope=scope)
    except AdapterError as error:
        print(
            json.dumps(
                {
                    "code": error.code,
                    "message": error.message,
                    "normalized_error": error.normalized_error,
                    "raw_error_summary": error.raw_error_summary,
                    "retryable": error.retryable,
                },
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 1

    payload = _summary(parser, state, scope) if args.summary else state
    print(_to_json(payload, pretty=args.pretty))
    return 0


@dataclass(frozen=True)
class _Scope:
    full: bool
    vlan_ids: list[int]
    interface_names: list[str]


def _summary(parser, state: dict, scope: _Scope) -> dict:
    profile = getattr(parser, "profile", None)
    return {
        "vendor": getattr(profile, "vendor", ""),
        "profile_name": getattr(profile, "profile_name", ""),
        "fixture_verified": bool(getattr(parser, "fixture_verified", False)),
        "production_ready": bool(getattr(parser, "production_ready", False)),
        "vlan_count": len(state.get("vlans", [])),
        "interface_count": len(state.get("interfaces", [])),
        "scope": {
            "full": scope.full,
            "vlan_ids": list(scope.vlan_ids),
            "interface_names": list(scope.interface_names),
        },
    }


def _to_json(payload: dict, *, pretty: bool = False) -> str:
    if pretty:
        return json.dumps(payload, indent=2, sort_keys=True)
    return json.dumps(payload, sort_keys=True)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="aria-underlay-state-parse",
        description="Validate NETCONF running-state XML with a fixture-verified parser.",
    )
    parser.add_argument("--vendor", required=True, help="Vendor name or enum value.")
    parser.add_argument(
        "--xml",
        required=True,
        type=argparse.FileType("r"),
        help="Path to captured NETCONF running XML.",
    )
    parser.add_argument("--full", action="store_true", help="Parse full observed state.")
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Pretty-print successful JSON output.",
    )
    parser.add_argument(
        "--summary",
        action="store_true",
        help="Print parser profile and resource counts instead of observed state.",
    )
    parser.add_argument(
        "--vlan",
        action="append",
        default=[],
        type=int,
        help="VLAN ID to include in parsed output. Repeat for multiple VLANs.",
    )
    parser.add_argument(
        "--interface",
        action="append",
        default=[],
        help="Interface name to include in parsed output. Repeat for multiple interfaces.",
    )
    return parser
