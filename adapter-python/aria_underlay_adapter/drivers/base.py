from __future__ import annotations

from collections.abc import Callable
from typing import Protocol


class DeviceDriver(Protocol):
    def get_capabilities(self, request): ...

    def get_current_state(self, request): ...

    def dry_run(self, device, desired_state): ...

    def prepare(self, request): ...

    def commit(self, tx_id, device, strategy=None, confirm_timeout_secs=120): ...

    def final_confirm(self, tx_id, device): ...

    def rollback(self, tx_id, device, strategy=None): ...

    def verify(self, tx_id, device, desired_state, scope=None): ...

    def recover(self, tx_id, device): ...

    def force_unlock(self, device, lock_owner, reason): ...


class DriverRegistry:
    def __init__(
        self,
        default_driver: DeviceDriver | None = None,
        driver_factory: Callable[[object], DeviceDriver] | None = None,
    ):
        self._default_driver = default_driver
        self._driver_factory = driver_factory

    def select(self, device) -> DeviceDriver:
        if self._driver_factory is not None:
            return self._driver_factory(device)
        if self._default_driver is None:
            raise RuntimeError("driver registry has no driver")
        return self._default_driver
