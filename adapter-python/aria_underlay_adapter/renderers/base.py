from __future__ import annotations

from typing import Protocol

from aria_underlay_adapter.renderers.xml import XmlElement


class VendorRenderer(Protocol):
    def render_vlan_create(self, vlan) -> XmlElement: ...

    def render_vlan_delete(self, vlan_id: int) -> XmlElement: ...

    def render_interface_update(self, interface) -> XmlElement: ...
