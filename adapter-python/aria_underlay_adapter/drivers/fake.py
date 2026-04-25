from __future__ import annotations

from aria_underlay_adapter.backends.mock_netconf import MockNetconfBackend
from aria_underlay_adapter.drivers.netconf_backed import NetconfBackedDriver


class FakeDriver(NetconfBackedDriver):
    def __init__(self, profile: str = "confirmed"):
        super().__init__(MockNetconfBackend(profile))
