from __future__ import annotations

from aria_underlay_adapter.renderers.xml import XmlElement


class HuaweiRenderer:
    """Structured XML renderer skeleton for Huawei VRP.

    The element names are intentionally minimal placeholders until Sprint 2
    confirms the exact YANG namespace and field mapping on real devices.
    """

    VLAN_NAMESPACE = "urn:aria:underlay:renderer:huawei:vlan:skeleton"
    IFACE_NAMESPACE = "urn:aria:underlay:renderer:huawei:interface:skeleton"

    def render_vlan_create(self, vlan) -> XmlElement:
        children = [XmlElement("id", namespace=self.VLAN_NAMESPACE, children=[str(vlan.vlan_id)])]
        if vlan.name:
            children.append(XmlElement("name", namespace=self.VLAN_NAMESPACE, children=[vlan.name]))
        if vlan.description:
            children.append(
                XmlElement("description", namespace=self.VLAN_NAMESPACE, children=[vlan.description])
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
            XmlElement("name", namespace=self.IFACE_NAMESPACE, children=[interface.name]),
            XmlElement("admin-state", namespace=self.IFACE_NAMESPACE, children=[interface.admin_state]),
        ]
        if interface.description:
            children.append(
                XmlElement(
                    "description",
                    namespace=self.IFACE_NAMESPACE,
                    children=[interface.description],
                )
            )
        children.append(_port_mode_element(interface.mode, self.IFACE_NAMESPACE))
        return XmlElement("interface", namespace=self.IFACE_NAMESPACE, children=children)


def _port_mode_element(mode: dict, namespace: str) -> XmlElement:
    kind = mode["kind"]
    if kind == "access":
        return XmlElement(
            "access",
            namespace=namespace,
            children=[
                XmlElement("vlan-id", namespace=namespace, children=[str(mode["access_vlan"])])
            ],
        )
    if kind == "trunk":
        children = []
        if mode.get("native_vlan") is not None:
            children.append(
                XmlElement(
                    "native-vlan",
                    namespace=namespace,
                    children=[str(mode["native_vlan"])],
                )
            )
        children.append(
            XmlElement(
                "allowed-vlans",
                namespace=namespace,
                children=[",".join(str(vlan) for vlan in mode["allowed_vlans"])],
            )
        )
        return XmlElement("trunk", namespace=namespace, children=children)
    raise ValueError(f"unknown port mode kind: {kind}")
