from __future__ import annotations

from typing import Any

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.state_parsers.base import RunningStateParser
from aria_underlay_adapter.state_parsers.h3c import H3cStateParser
from aria_underlay_adapter.state_parsers.huawei import HuaweiStateParser


VENDOR_UNSPECIFIED = 0
VENDOR_HUAWEI = 1
VENDOR_H3C = 2
VENDOR_CISCO = 3
VENDOR_RUIJIE = 4
VENDOR_UNKNOWN = 100

_VENDOR_NAMES = {
    VENDOR_UNSPECIFIED: "unspecified",
    VENDOR_HUAWEI: "huawei",
    VENDOR_H3C: "h3c",
    VENDOR_CISCO: "cisco",
    VENDOR_RUIJIE: "ruijie",
    VENDOR_UNKNOWN: "unknown",
}

_VENDOR_ALIASES = {
    "huawei": VENDOR_HUAWEI,
    "vendor_huawei": VENDOR_HUAWEI,
    "h3c": VENDOR_H3C,
    "vendor_h3c": VENDOR_H3C,
    "cisco": VENDOR_CISCO,
    "vendor_cisco": VENDOR_CISCO,
    "ruijie": VENDOR_RUIJIE,
    "vendor_ruijie": VENDOR_RUIJIE,
    "unknown": VENDOR_UNKNOWN,
    "vendor_unknown": VENDOR_UNKNOWN,
    "unspecified": VENDOR_UNSPECIFIED,
    "vendor_unspecified": VENDOR_UNSPECIFIED,
}

_PARSERS = {
    VENDOR_HUAWEI: HuaweiStateParser,
    VENDOR_H3C: H3cStateParser,
}


def state_parser_for_vendor(
    vendor: Any,
    *,
    allow_skeleton: bool = False,
    allow_fixture_verified: bool = False,
    model_hint: str | None = None,
) -> RunningStateParser:
    vendor_value = _vendor_value(vendor)
    parser_type = _PARSERS.get(vendor_value)
    vendor_name = _vendor_name(vendor_value)

    if parser_type is None:
        raise AdapterError(
            code="STATE_PARSER_VENDOR_UNSUPPORTED",
            message=f"no NETCONF state parser is registered for vendor {vendor_name}",
            normalized_error="state parser vendor unsupported",
            raw_error_summary=f"vendor={vendor_name} ({vendor_value})",
            retryable=False,
        )

    try:
        parser = parser_type(model_hint=model_hint)
    except TypeError:
        parser = parser_type()
    if getattr(parser, "production_ready", False):
        return parser
    if allow_skeleton:
        return parser
    if allow_fixture_verified and getattr(parser, "fixture_verified", False):
        return parser
    if not getattr(parser, "production_ready", False):
        raise AdapterError(
            code="STATE_PARSER_NOT_PRODUCTION_READY",
            message=f"NETCONF state parser for vendor {vendor_name} is not production ready",
            normalized_error="state parser not production ready",
            raw_error_summary=(
                f"vendor={vendor_name} parser={parser_type.__name__} "
                "is not production-ready; refusing production selection"
            ),
            retryable=False,
        )

    return parser


def _vendor_value(vendor: Any) -> int:
    if isinstance(vendor, str):
        key = vendor.strip().lower()
        if key in _VENDOR_ALIASES:
            return _VENDOR_ALIASES[key]
        try:
            return int(key)
        except ValueError:
            return VENDOR_UNKNOWN

    if hasattr(vendor, "value"):
        return int(vendor.value)

    try:
        return int(vendor)
    except (TypeError, ValueError):
        return VENDOR_UNKNOWN


def _vendor_name(vendor_value: int) -> str:
    return _VENDOR_NAMES.get(vendor_value, f"vendor-{vendor_value}")
