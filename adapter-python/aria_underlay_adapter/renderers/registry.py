from __future__ import annotations

from typing import Any

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.base import VendorRenderer
from aria_underlay_adapter.renderers.h3c import H3cRenderer
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer


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

_RENDERERS = {
    VENDOR_HUAWEI: HuaweiRenderer,
    VENDOR_H3C: H3cRenderer,
}


def renderer_for_vendor(
    vendor: Any,
    *,
    allow_skeleton: bool = False,
) -> VendorRenderer:
    vendor_value = _vendor_value(vendor)
    renderer_type = _RENDERERS.get(vendor_value)
    vendor_name = _vendor_name(vendor_value)

    if renderer_type is None:
        raise AdapterError(
            code="RENDERER_VENDOR_UNSUPPORTED",
            message=f"no NETCONF renderer is registered for vendor {vendor_name}",
            normalized_error="renderer vendor unsupported",
            raw_error_summary=f"vendor={vendor_name} ({vendor_value})",
            retryable=False,
        )

    renderer = renderer_type()
    if not allow_skeleton and not getattr(renderer, "production_ready", False):
        raise AdapterError(
            code="RENDERER_NOT_PRODUCTION_READY",
            message=f"NETCONF renderer for vendor {vendor_name} is not production ready",
            normalized_error="renderer not production ready",
            raw_error_summary=(
                f"vendor={vendor_name} renderer={renderer_type.__name__} "
                "is skeleton-only; refusing production selection"
            ),
            retryable=False,
        )

    return renderer


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
