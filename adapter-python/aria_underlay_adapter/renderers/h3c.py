from __future__ import annotations

from dataclasses import dataclass
import re

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.normalization import admin_state_to_text
from aria_underlay_adapter.renderers.xml import NETCONF_BASE_NAMESPACE
from aria_underlay_adapter.renderers.xml import XmlElement
from aria_underlay_adapter.renderers.xml import qualified_attr
from aria_underlay_adapter.renderers.xml import render_xml


H3C_COMWARE_CONFIG_NAMESPACE = "http://www.h3c.com/netconf/config:1.0"
_H3C_INTERFACE_RE = re.compile(
    r"^(?:GigabitEthernet|Ten-GigabitEthernet|FortyGigE)1/0/([1-9][0-9]*)(?:\.\d+)?$"
)


@dataclass(frozen=True)
class H3cRendererProfile:
    vendor: str = "h3c"
    profile_name: str = "comware7-vlan-real"
    production_ready: bool = True


class H3cRenderer:
    """Production renderer for H3C Comware VLAN and port mode edits."""

    profile = H3cRendererProfile()

    @property
    def production_ready(self) -> bool:
        return self.profile.production_ready

    @property
    def VLAN_NAMESPACE(self) -> str:
        return H3C_COMWARE_CONFIG_NAMESPACE

    @property
    def IFACE_NAMESPACE(self) -> str:
        return H3C_COMWARE_CONFIG_NAMESPACE

    def render_edit_config(self, desired_state) -> str:
        vlan_nodes = [
            self.render_vlan_create(vlan)
            for vlan in getattr(desired_state, "vlans", [])
        ]
        access_nodes = []
        trunk_nodes = []
        for interface in getattr(desired_state, "interfaces", []):
            kind = _mode_kind(_field(interface, "mode"))
            node = self.render_interface_update(interface)
            if kind == "access":
                access_nodes.append(node)
            elif kind == "trunk":
                trunk_nodes.append(node)

        vlan_children = []
        if vlan_nodes:
            vlan_children.append(
                XmlElement("VLANs", namespace=self.VLAN_NAMESPACE, children=vlan_nodes)
            )
        if access_nodes:
            vlan_children.append(
                XmlElement(
                    "AccessInterfaces",
                    namespace=self.VLAN_NAMESPACE,
                    children=access_nodes,
                )
            )
        if trunk_nodes:
            vlan_children.append(
                XmlElement(
                    "TrunkInterfaces",
                    namespace=self.VLAN_NAMESPACE,
                    children=trunk_nodes,
                )
            )
        if not vlan_children:
            raise AdapterError(
                code="EMPTY_DESIRED_STATE",
                message="desired state contains no VLAN or interface changes",
                normalized_error="empty desired state",
                raw_error_summary="renderer refused to produce an empty edit-config payload",
                retryable=False,
            )

        return render_xml(
            XmlElement(
                "config",
                children=[
                    XmlElement(
                        "top",
                        namespace=self.VLAN_NAMESPACE,
                        children=[
                            XmlElement(
                                "VLAN",
                                namespace=self.VLAN_NAMESPACE,
                                children=vlan_children,
                            )
                        ],
                    )
                ],
            )
        )

    def render_vlan_create(self, vlan) -> XmlElement:
        vlan_id = _validate_vlan_id(_field(vlan, "vlan_id"), "vlan.vlan_id")
        if _optional_text(vlan, "description") is not None:
            raise ValueError("H3C VLAN description is not supported yet")

        children = [XmlElement("ID", namespace=self.VLAN_NAMESPACE, children=[str(vlan_id)])]
        name = _optional_text(vlan, "name")
        if name is not None:
            children.append(XmlElement("Name", namespace=self.VLAN_NAMESPACE, children=[name]))
        return XmlElement("VLANID", namespace=self.VLAN_NAMESPACE, children=children)

    def render_vlan_delete(self, vlan_id: int) -> XmlElement:
        vlan_id = _validate_vlan_id(vlan_id, "vlan_id")
        return XmlElement(
            "VLANID",
            namespace=self.VLAN_NAMESPACE,
            attributes={qualified_attr("operation", NETCONF_BASE_NAMESPACE): "delete"},
            children=[XmlElement("ID", namespace=self.VLAN_NAMESPACE, children=[str(vlan_id)])],
        )

    def render_interface_update(self, interface) -> XmlElement:
        name = _required_text(interface, "name")
        if _optional_text(interface, "description") is not None:
            raise ValueError("H3C interface description is not supported in VLAN renderer")
        _validate_admin_state(_field(interface, "admin_state"))

        ifindex = _interface_ifindex(name)
        mode = _field(interface, "mode")
        kind = _mode_kind(mode)
        children = [
            XmlElement("IfIndex", namespace=self.IFACE_NAMESPACE, children=[str(ifindex)])
        ]
        if kind == "access":
            children.append(
                XmlElement(
                    "PVID",
                    namespace=self.IFACE_NAMESPACE,
                    children=[
                        str(
                            _validate_vlan_id(
                                _optional_field(mode, "access_vlan"),
                                "mode.access_vlan",
                            )
                        )
                    ],
                )
            )
        elif kind == "trunk":
            if _optional_field(mode, "native_vlan") is not None:
                raise ValueError("H3C trunk native_vlan is not supported yet")
            allowed_vlans = [
                _validate_vlan_id(vlan, "mode.allowed_vlans")
                for vlan in _repeated_field(mode, "allowed_vlans")
            ]
            if not allowed_vlans:
                raise ValueError("trunk port mode requires native_vlan or allowed_vlans")
            if len(set(allowed_vlans)) != len(allowed_vlans):
                raise ValueError("trunk port mode contains duplicate allowed_vlans")
            children.append(
                XmlElement(
                    "PermitVlanList",
                    namespace=self.IFACE_NAMESPACE,
                    children=[_format_vlan_ranges(allowed_vlans)],
                )
            )

        return XmlElement("Interface", namespace=self.IFACE_NAMESPACE, children=children)


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


def _required_text(message, name: str) -> str:
    value = _optional_text(message, name)
    if value is None:
        raise ValueError(f"{name} is required")
    return value


def _optional_text(message, name: str) -> str | None:
    value = _optional_field(message, name)
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _validate_vlan_id(value, field: str) -> int:
    if value is None:
        raise ValueError(f"{field} is required")
    try:
        vlan_id = int(value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{field} must be an integer VLAN ID") from exc
    if vlan_id < 1 or vlan_id > 4094:
        raise ValueError(f"{field} must be in range 1..4094")
    return vlan_id


def _mode_kind(mode) -> str:
    kind = _field(mode, "kind")
    normalized_kind = kind.strip().lower() if isinstance(kind, str) else kind
    if normalized_kind in {"access", 1}:
        return "access"
    if normalized_kind in {"trunk", 2}:
        return "trunk"
    raise ValueError(f"unknown port mode kind: {kind}")


def _interface_ifindex(name: str) -> int:
    if not name.strip():
        raise ValueError("name is required")
    match = _H3C_INTERFACE_RE.fullmatch(name.strip())
    if match is None:
        raise ValueError(f"unsupported H3C interface name: {name}")
    return int(match.group(1))


def _validate_admin_state(value) -> None:
    state = admin_state_to_text(value)
    if state != "up":
        raise ValueError("H3C admin_state down is not supported in VLAN renderer")


def _format_vlan_ranges(vlan_ids: list[int]) -> str:
    values = sorted(vlan_ids)
    ranges = []
    start = values[0]
    previous = values[0]
    for vlan_id in values[1:]:
        if vlan_id == previous + 1:
            previous = vlan_id
            continue
        ranges.append(_format_vlan_range(start, previous))
        start = vlan_id
        previous = vlan_id
    ranges.append(_format_vlan_range(start, previous))
    return ",".join(ranges)


def _format_vlan_range(start: int, end: int) -> str:
    if start == end:
        return str(start)
    return f"{start}-{end}"
