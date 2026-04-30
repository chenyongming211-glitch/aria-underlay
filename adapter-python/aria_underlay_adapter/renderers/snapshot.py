from __future__ import annotations

import argparse
import json
import sys
from types import SimpleNamespace

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.registry import renderer_for_vendor


def main(argv=None) -> int:
    args = _parser().parse_args(argv)
    try:
        desired_state = _load_desired_state(args.desired_state)
        renderer = renderer_for_vendor(args.vendor, allow_skeleton=True)
        xml = renderer.render_edit_config(desired_state)
    except AdapterError as error:
        _print_error(_error_payload(error))
        return 1
    except ValueError as error:
        _print_error(
            {
                "code": "RENDER_SNAPSHOT_FAILED",
                "message": "failed to render desired-state snapshot",
                "normalized_error": "render_snapshot_failed",
                "raw_error_summary": str(error),
                "retryable": False,
            }
        )
        return 1

    profile = getattr(renderer, "profile", None)
    payload = {
        "vendor": getattr(profile, "vendor", ""),
        "profile_name": getattr(profile, "profile_name", ""),
        "production_ready": bool(getattr(renderer, "production_ready", False)),
        "vlan_count": len(desired_state.vlans),
        "interface_count": len(desired_state.interfaces),
        "xml": xml,
    }
    print(_to_json(payload, pretty=args.pretty))
    return 0


def _load_desired_state(path) -> SimpleNamespace:
    try:
        data = json.load(path)
    except (OSError, json.JSONDecodeError) as error:
        raise _input_error(str(error)) from error
    if not isinstance(data, dict):
        raise _input_error("desired state must be a JSON object")

    vlans = data.get("vlans", [])
    interfaces = data.get("interfaces", [])
    if not isinstance(vlans, list):
        raise _input_error("desired state vlans must be a list")
    if not isinstance(interfaces, list):
        raise _input_error("desired state interfaces must be a list")
    return SimpleNamespace(vlans=vlans, interfaces=interfaces)


def _input_error(summary: str) -> AdapterError:
    return AdapterError(
        code="RENDER_SNAPSHOT_INPUT_INVALID",
        message="invalid desired-state snapshot input",
        normalized_error="render_snapshot_input_invalid",
        raw_error_summary=summary,
        retryable=False,
    )


def _error_payload(error: AdapterError) -> dict:
    return {
        "code": error.code,
        "message": error.message,
        "normalized_error": error.normalized_error,
        "raw_error_summary": error.raw_error_summary,
        "retryable": error.retryable,
    }


def _print_error(payload: dict) -> None:
    print(json.dumps(payload, sort_keys=True), file=sys.stderr)


def _to_json(payload: dict, *, pretty: bool = False) -> str:
    if pretty:
        return json.dumps(payload, indent=2, sort_keys=True)
    return json.dumps(payload, sort_keys=True)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="aria-underlay-render-snapshot",
        description="Render desired-state JSON through an offline skeleton renderer.",
    )
    parser.add_argument("--vendor", required=True, help="Vendor name or enum value.")
    parser.add_argument(
        "--desired-state",
        required=True,
        type=argparse.FileType("r"),
        help="Path to desired-state JSON.",
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Pretty-print successful JSON output.",
    )
    return parser
