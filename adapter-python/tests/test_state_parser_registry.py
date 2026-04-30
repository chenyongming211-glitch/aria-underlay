import pytest

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.state_parsers.h3c import H3cStateParser
from aria_underlay_adapter.state_parsers.huawei import HuaweiStateParser
from aria_underlay_adapter.state_parsers.registry import (
    VENDOR_CISCO,
    VENDOR_H3C,
    VENDOR_HUAWEI,
    VENDOR_UNKNOWN,
    state_parser_for_vendor,
)


@pytest.mark.parametrize(
    ("vendor", "parser_type"),
    [
        (VENDOR_HUAWEI, HuaweiStateParser),
        ("huawei", HuaweiStateParser),
        ("VENDOR_HUAWEI", HuaweiStateParser),
        (VENDOR_H3C, H3cStateParser),
        ("h3c", H3cStateParser),
        ("VENDOR_H3C", H3cStateParser),
    ],
)
def test_state_parser_registry_can_return_skeleton_when_explicitly_allowed(
    vendor,
    parser_type,
):
    parser = state_parser_for_vendor(vendor, allow_skeleton=True)

    assert isinstance(parser, parser_type)
    assert parser.production_ready is False


@pytest.mark.parametrize("vendor", [VENDOR_HUAWEI, "huawei", VENDOR_H3C, "h3c"])
def test_state_parser_registry_rejects_skeletons_by_default(vendor):
    with pytest.raises(AdapterError) as exc:
        state_parser_for_vendor(vendor)

    assert exc.value.code == "STATE_PARSER_NOT_PRODUCTION_READY"


@pytest.mark.parametrize("vendor", [VENDOR_HUAWEI, "huawei", VENDOR_H3C, "h3c"])
def test_state_parser_registry_allows_fixture_verified_parsers_when_explicit(vendor):
    parser = state_parser_for_vendor(vendor, allow_fixture_verified=True)

    assert parser.production_ready is False
    assert parser.fixture_verified is True


@pytest.mark.parametrize("vendor", [VENDOR_CISCO, VENDOR_UNKNOWN, "ruijie", "unknown"])
def test_state_parser_registry_rejects_unregistered_vendors(vendor):
    with pytest.raises(AdapterError) as exc:
        state_parser_for_vendor(vendor)

    assert exc.value.code == "STATE_PARSER_VENDOR_UNSUPPORTED"
