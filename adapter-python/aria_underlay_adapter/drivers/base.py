from __future__ import annotations

from typing import Protocol


class DeviceDriver(Protocol):
    def get_capabilities(self, request): ...

    def get_current_state(self, request): ...

    def dry_run(self, device, desired_state): ...

    def prepare(self, request): ...

    def commit(self, tx_id, device): ...

    def rollback(self, tx_id, device): ...

    def verify(self, tx_id, device, desired_state): ...

    def recover(self, tx_id, device): ...

    def force_unlock(self, device, lock_owner, reason): ...


class DriverRegistry:
    def __init__(self, default_driver: DeviceDriver):
        self._default_driver = default_driver

    def select(self, device) -> DeviceDriver:
        return self._default_driver
