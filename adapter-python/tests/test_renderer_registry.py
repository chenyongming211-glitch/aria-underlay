import pytest

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.h3c import H3cRenderer
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer
from aria_underlay_adapter.renderers.registry import (
    VENDOR_CISCO,
    VENDOR_H3C,
    VENDOR_HUAWEI,
    VENDOR_UNKNOWN,
    renderer_for_vendor,
)


@pytest.mark.parametrize(
    ("vendor", "renderer_type"),
    [
        (VENDOR_HUAWEI, HuaweiRenderer),
        ("huawei", HuaweiRenderer),
        ("VENDOR_HUAWEI", HuaweiRenderer),
        (VENDOR_H3C, H3cRenderer),
        ("h3c", H3cRenderer),
        ("VENDOR_H3C", H3cRenderer),
    ],
)
def test_renderer_registry_can_return_skeleton_when_explicitly_allowed(
    vendor,
    renderer_type,
):
    renderer = renderer_for_vendor(vendor, allow_skeleton=True)

    assert isinstance(renderer, renderer_type)
    assert renderer.production_ready is False


@pytest.mark.parametrize("vendor", [VENDOR_HUAWEI, "huawei", VENDOR_H3C, "h3c"])
def test_renderer_registry_rejects_skeletons_by_default(vendor):
    with pytest.raises(AdapterError) as exc:
        renderer_for_vendor(vendor)

    assert exc.value.code == "RENDERER_NOT_PRODUCTION_READY"


@pytest.mark.parametrize("vendor", [VENDOR_CISCO, VENDOR_UNKNOWN, "ruijie", "unknown"])
def test_renderer_registry_rejects_unregistered_vendors(vendor):
    with pytest.raises(AdapterError) as exc:
        renderer_for_vendor(vendor)

    assert exc.value.code == "RENDERER_VENDOR_UNSUPPORTED"
