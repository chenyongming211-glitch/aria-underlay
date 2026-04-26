from __future__ import annotations

from typing import Protocol

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.xml import XmlElement
from aria_underlay_adapter.renderers.xml import render_xml


class VendorRenderer(Protocol):
    production_ready: bool

    def render_vlan_create(self, vlan) -> XmlElement: ...

    def render_vlan_delete(self, vlan_id: int) -> XmlElement: ...

    def render_interface_update(self, interface) -> XmlElement: ...

    def render_edit_config(self, desired_state) -> str: ...


def render_edit_config_document(renderer: VendorRenderer, desired_state) -> str:
    children = []
    for vlan in getattr(desired_state, "vlans", []):
        children.append(renderer.render_vlan_create(vlan))
    for interface in getattr(desired_state, "interfaces", []):
        children.append(renderer.render_interface_update(interface))

    if not children:
        raise AdapterError(
            code="EMPTY_DESIRED_STATE",
            message="desired state contains no VLAN or interface changes",
            normalized_error="empty desired state",
            raw_error_summary="renderer refused to produce an empty edit-config payload",
            retryable=False,
        )

    return render_xml(XmlElement("config", children=children))
