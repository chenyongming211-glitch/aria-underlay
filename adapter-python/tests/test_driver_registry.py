from aria_underlay_adapter.drivers.base import DriverRegistry


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
