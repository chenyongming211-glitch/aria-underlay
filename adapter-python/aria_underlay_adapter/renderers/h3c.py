from __future__ import annotations

from aria_underlay_adapter.renderers.base import render_edit_config_document
from aria_underlay_adapter.renderers.xml import XmlElement


class H3cRenderer:
    """Structured XML renderer skeleton for H3C Comware.

    The namespace and element names are placeholders until Sprint 2 validates
    them against Comware NETCONF/YANG documentation or a real device.
    """

    production_ready = False
    VLAN_NAMESPACE = "urn:aria:underlay:renderer:h3c:vlan:skeleton"
    IFACE_NAMESPACE = "urn:aria:underlay:renderer:h3c:interface:skeleton"

    def render_edit_config(self, desired_state) -> str:
        return render_edit_config_document(self, desired_state)

    def render_vlan_create(self, vlan) -> XmlElement:
        children = [XmlElement("id", namespace=self.VLAN_NAMESPACE, children=[str(_field(vlan, "vlan_id"))])]
        name = _optional_field(vlan, "name")
        description = _optional_field(vlan, "description")
        if name:
            children.append(XmlElement("name", namespace=self.VLAN_NAMESPACE, children=[name]))
        if description:
            children.append(
                XmlElement("description", namespace=self.VLAN_NAMESPACE, children=[description])
            )
        return XmlElement("vlan", namespace=self.VLAN_NAMESPACE, children=children)

    def render_vlan_delete(self, vlan_id: int) -> XmlElement:
        return XmlElement(
            "vlan",
            namespace=self.VLAN_NAMESPACE,
            attributes={"operation": "delete"},
            children=[XmlElement("id", namespace=self.VLAN_NAMESPACE, children=[str(vlan_id)])],
        )

    def render_interface_update(self, interface) -> XmlElement:
        children = [
            XmlElement("name", namespace=self.IFACE_NAMESPACE, children=[_field(interface, "name")]),
            XmlElement(
                "admin-state",
                namespace=self.IFACE_NAMESPACE,
                children=[_admin_state_text(_field(interface, "admin_state"))],
            ),
        ]
        description = _optional_field(interface, "description")
        if description:
            children.append(
                XmlElement(
                    "description",
                    namespace=self.IFACE_NAMESPACE,
                    children=[description],
                )
            )
        children.append(_port_mode_element(_field(interface, "mode"), self.IFACE_NAMESPACE))
        return XmlElement("interface", namespace=self.IFACE_NAMESPACE, children=children)


def _port_mode_element(mode: dict, namespace: str) -> XmlElement:
    kind = _field(mode, "kind")
    if kind in {"access", "ACCESS", 1}:
        access_vlan = _optional_field(mode, "access_vlan")
        if access_vlan is None:
            raise ValueError("access_vlan is required for access port mode")
        return XmlElement(
            "access",
            namespace=namespace,
            children=[
                XmlElement(
                    "vlan-id",
                    namespace=namespace,
                    children=[str(access_vlan)],
                )
            ],
        )
    if kind in {"trunk", "TRUNK", 2}:
        children = []
        native_vlan = _optional_field(mode, "native_vlan")
        if native_vlan is not None:
            children.append(
                XmlElement(
                    "native-vlan",
                    namespace=namespace,
                    children=[str(native_vlan)],
                )
            )
        allowed_vlans = _repeated_field(mode, "allowed_vlans")
        children.append(
            XmlElement(
                "allowed-vlans",
                namespace=namespace,
                children=[",".join(str(vlan) for vlan in allowed_vlans)],
            )
        )
        return XmlElement("trunk", namespace=namespace, children=children)
    raise ValueError(f"unknown port mode kind: {kind}")


def _field(message, name):
    if isinstance(message, dict):
        return message[name]
    return getattr(message, name)


def _optional_field(message, name):
    if isinstance(message, dict):
        return message.get(name)
    if hasattr(message, "HasField"):
        try:
            return getattr(message, name) if message.HasField(name) else None
        except ValueError:
            return getattr(message, name)
    return getattr(message, name, None)


def _repeated_field(message, name):
    if isinstance(message, dict):
        return list(message.get(name, []))
    return list(getattr(message, name, []))


def _admin_state_text(value) -> str:
    if value in {"up", "UP", 1}:
        return "up"
    if value in {"down", "DOWN", 2}:
        return "down"
    return str(value).lower()
