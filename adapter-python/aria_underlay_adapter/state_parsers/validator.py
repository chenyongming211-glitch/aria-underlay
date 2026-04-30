from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.state_parsers.registry import state_parser_for_vendor


def main(argv=None) -> int:
    args = _parser().parse_args(argv)
    if args.manifest:
        invalid = _manifest_argument_conflict(args)
        if invalid:
            _print_error(
                {
                    "code": "STATE_PARSER_ARGUMENT_INVALID",
                    "message": "--manifest cannot be combined with single-sample arguments",
                    "normalized_error": "invalid_argument",
                    "raw_error_summary": invalid,
                    "retryable": False,
                }
            )
            return 1
        return _run_manifest(args)

    if not args.vendor or not args.xml:
        _print_error(
            {
                "code": "STATE_PARSER_ARGUMENT_INVALID",
                "message": "single-sample validation requires --vendor and --xml",
                "normalized_error": "invalid_argument",
                "raw_error_summary": "missing required --vendor or --xml",
                "retryable": False,
            }
        )
        return 1

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
        _print_error(_error_payload(error))
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


def _manifest_argument_conflict(args) -> str:
    for name in ("vendor", "xml", "full", "summary", "vlan", "interface"):
        value = getattr(args, name)
        if value:
            option = "--interface" if name == "interface" else f"--{name}"
            return f"--manifest cannot be combined with {option}"
    return ""


def _run_manifest(args) -> int:
    try:
        samples = _load_manifest_samples(Path(args.manifest))
    except AdapterError as error:
        _print_error(_error_payload(error))
        return 1

    results = [_validate_manifest_sample(sample) for sample in samples]
    failed = len([result for result in results if not result["ok"]])
    report = {
        "ok": failed == 0,
        "sample_count": len(results),
        "passed": len(results) - failed,
        "failed": failed,
        "samples": results,
    }
    print(_to_json(report, pretty=args.pretty))
    return 0 if report["ok"] else 1


@dataclass(frozen=True)
class _ManifestSample:
    name: str
    vendor: str
    xml_path: Path
    scope: _Scope


def _load_manifest_samples(manifest_path: Path) -> list[_ManifestSample]:
    try:
        data = json.loads(manifest_path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise _manifest_error(str(error)) from error

    if not isinstance(data, dict):
        raise _manifest_error("manifest must be a JSON object")
    samples = data.get("samples")
    if not isinstance(samples, list):
        raise _manifest_error("samples must be a list")

    return [
        _manifest_sample_from_dict(manifest_path.parent, index, sample)
        for index, sample in enumerate(samples)
    ]


def _manifest_sample_from_dict(
    manifest_dir: Path, index: int, sample: object
) -> _ManifestSample:
    if not isinstance(sample, dict):
        raise _manifest_error(f"samples[{index}] must be an object")

    name = _required_string(sample, "name", index)
    vendor = _required_string(sample, "vendor", index)
    xml = _required_string(sample, "xml", index)
    xml_path = Path(xml)
    if not xml_path.is_absolute():
        xml_path = manifest_dir / xml_path

    scope_data = sample.get("scope", {})
    if not isinstance(scope_data, dict):
        raise _manifest_error(f"samples[{index}].scope must be an object")
    vlan_ids = scope_data.get("vlans", [])
    interface_names = scope_data.get("interfaces", [])
    if not isinstance(vlan_ids, list):
        raise _manifest_error(f"samples[{index}].scope.vlans must be a list")
    if not isinstance(interface_names, list):
        raise _manifest_error(f"samples[{index}].scope.interfaces must be a list")
    for vlan_index, vlan_id in enumerate(vlan_ids):
        if not isinstance(vlan_id, int):
            raise _manifest_error(
                f"samples[{index}].scope.vlans[{vlan_index}] must be an integer"
            )
    for interface_index, interface_name in enumerate(interface_names):
        if not isinstance(interface_name, str) or not interface_name:
            raise _manifest_error(
                "samples[{}].scope.interfaces[{}] must be a non-empty string".format(
                    index, interface_index
                )
            )

    return _ManifestSample(
        name=name,
        vendor=vendor,
        xml_path=xml_path,
        scope=_Scope(
            full=not vlan_ids and not interface_names,
            vlan_ids=vlan_ids,
            interface_names=interface_names,
        ),
    )


def _required_string(sample: dict, key: str, index: int) -> str:
    value = sample.get(key)
    if not isinstance(value, str) or not value:
        raise _manifest_error(f"samples[{index}].{key} must be a non-empty string")
    return value


def _validate_manifest_sample(sample: _ManifestSample) -> dict:
    try:
        parser = state_parser_for_vendor(sample.vendor, allow_fixture_verified=True)
        state = parser.parse_running(sample.xml_path.read_text(), scope=sample.scope)
    except AdapterError as error:
        return {
            "name": sample.name,
            "ok": False,
            "error": _error_payload(error),
        }
    except OSError as error:
        return {
            "name": sample.name,
            "ok": False,
            "error": {
                "code": "STATE_PARSER_SAMPLE_XML_UNREADABLE",
                "message": "failed to read NETCONF running XML sample",
                "normalized_error": "sample_xml_unreadable",
                "raw_error_summary": str(error),
                "retryable": False,
            },
        }

    return {
        "name": sample.name,
        "ok": True,
        "summary": _summary(parser, state, sample.scope),
    }


def _manifest_error(summary: str) -> AdapterError:
    return AdapterError(
        code="STATE_PARSER_MANIFEST_INVALID",
        message="invalid state parser validation manifest",
        normalized_error="manifest_invalid",
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
        prog="aria-underlay-state-parse",
        description="Validate NETCONF running-state XML with a fixture-verified parser.",
    )
    parser.add_argument(
        "--manifest",
        help="Path to a JSON manifest of NETCONF running XML samples.",
    )
    parser.add_argument("--vendor", help="Vendor name or enum value.")
    parser.add_argument(
        "--xml",
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
