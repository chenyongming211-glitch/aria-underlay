from aria_underlay_adapter.drivers.base import DriverRegistry
from aria_underlay_adapter.drivers.cisco import CiscoDriver
from aria_underlay_adapter.drivers.h3c import H3cDriver
from aria_underlay_adapter.drivers.huawei import HuaweiDriver
from aria_underlay_adapter.drivers.legacy_cli import LegacyCliDriver
from aria_underlay_adapter.drivers.ruijie import RuijieDriver


class _Driver:
    pass


class _Device:
    device_id = "leaf-a"


def test_driver_registry_uses_default_driver():
    driver = _Driver()
    registry = DriverRegistry(default_driver=driver)

    assert registry.select(_Device()) is driver


def test_driver_registry_uses_device_factory():
    registry = DriverRegistry(driver_factory=lambda device: f"driver:{device.device_id}")

    assert registry.select(_Device()) == "driver:leaf-a"


def test_vendor_drivers_are_constructable_but_operations_fail_closed():
    for driver in [CiscoDriver, H3cDriver, HuaweiDriver, LegacyCliDriver, RuijieDriver]:
        instance = driver()
        try:
            instance.get_capabilities(None)
        except NotImplementedError as error:
            assert driver.__name__.removesuffix("Driver") in str(error)
            continue
        raise AssertionError(f"{driver.__name__} should fail closed for unsupported operations")
