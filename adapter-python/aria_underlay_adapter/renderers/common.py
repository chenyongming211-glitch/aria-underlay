from __future__ import annotations

from dataclasses import dataclass
import re

from aria_underlay_adapter.normalization import admin_state_to_text
from aria_underlay_adapter.renderers.base import render_edit_config_document
from aria_underlay_adapter.renderers.xml import NETCONF_BASE_NAMESPACE
from aria_underlay_adapter.renderers.xml import XmlElement
from aria_underlay_adapter.renderers.xml import qualified_attr


_PROFILE_TOKEN_RE = re.compile(r"^[a-z0-9][a-z0-9_.-]*$")
_NAMESPACE_SCHEMES = ("urn:", "http://", "https://")


@dataclass(frozen=True)
class RendererNamespaceProfile:
    vendor: str
    profile_name: str
    vlan_namespace: str
    interface_namespace: str
    production_ready: bool = False

    def __post_init__(self) -> None:
        _validate_token(self.vendor, "vendor")
        _validate_token(self.profile_name, "profile_name")
        _validate_namespace(self.vlan_namespace, "vlan_namespace")
        _validate_namespace(self.interface_namespace, "interface_namespace")
        if self.vlan_namespace == self.interface_namespace:
            raise ValueError("vlan_namespace and interface_namespace must be distinct")
        if f":{self.vendor}:" not in self.vlan_namespace:
            raise ValueError("vlan_namespace must include the renderer vendor token")
        if f":{self.vendor}:" not in self.interface_namespace:
            raise ValueError("interface_namespace must include the renderer vendor token")
        if self.production_ready and (
            self.profile_name.endswith("-skeleton")
            or self.vlan_namespace.endswith(":skeleton")
            or self.interface_namespace.endswith(":skeleton")
        ):
            raise ValueError("production_ready profile cannot use skeleton markers")


class StructuredSkeletonRenderer:
    """Shared structured renderer for vendor skeleton profiles.

    Skeleton renderers intentionally stay production_ready=False until their
    namespace profile and field mapping are validated against real devices.
    """

    profile: RendererNamespaceProfile

    @property
    def production_ready(self) -> bool:
        return self.profile.production_ready

    @property
    def VLAN_NAMESPACE(self) -> str:
        return self.profile.vlan_namespace

    @property
    def IFACE_NAMESPACE(self) -> str:
        return self.profile.interface_namespace

    def render_edit_config(self, desired_state) -> str:
        return render_edit_config_document(self, desired_state)

    def render_vlan_create(self, vlan) -> XmlElement:
        vlan_id = _validate_vlan_id(_field(vlan, "vlan_id"), "vlan.vlan_id")
        children = [
            XmlElement("id", namespace=self.VLAN_NAMESPACE, children=[str(vlan_id)])
        ]
        name = _optional_text(vlan, "name")
        description = _optional_text(vlan, "description")
        if name:
            children.append(XmlElement("name", namespace=self.VLAN_NAMESPACE, children=[name]))
        if description:
            children.append(
                XmlElement("description", namespace=self.VLAN_NAMESPACE, children=[description])
            )
        return XmlElement("vlan", namespace=self.VLAN_NAMESPACE, children=children)

    def render_vlan_delete(self, vlan_id: int) -> XmlElement:
        vlan_id = _validate_vlan_id(vlan_id, "vlan_id")
        return XmlElement(
            "vlan",
            namespace=self.VLAN_NAMESPACE,
            attributes={qualified_attr("operation", NETCONF_BASE_NAMESPACE): "delete"},
            children=[XmlElement("id", namespace=self.VLAN_NAMESPACE, children=[str(vlan_id)])],
        )

    def render_interface_update(self, interface) -> XmlElement:
        name = _required_text(interface, "name")
        children = [
            XmlElement("name", namespace=self.IFACE_NAMESPACE, children=[name]),
            XmlElement(
                "admin-state",
                namespace=self.IFACE_NAMESPACE,
                children=[_admin_state_text(_field(interface, "admin_state"))],
            ),
        ]
        description = _optional_text(interface, "description")
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
    normalized_kind = kind.strip().lower() if isinstance(kind, str) else kind
    if normalized_kind in {"access", 1}:
        access_vlan = _validate_vlan_id(
            _optional_field(mode, "access_vlan"),
            "mode.access_vlan",
        )
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
    if normalized_kind in {"trunk", 2}:
        children = []
        native_vlan = _optional_field(mode, "native_vlan")
        if native_vlan is not None:
            children.append(
                XmlElement(
                    "native-vlan",
                    namespace=namespace,
                    children=[str(_validate_vlan_id(native_vlan, "mode.native_vlan"))],
                )
            )
        allowed_vlans = [
            _validate_vlan_id(vlan, "mode.allowed_vlans")
            for vlan in _repeated_field(mode, "allowed_vlans")
        ]
        if not allowed_vlans and native_vlan is None:
            raise ValueError("trunk port mode requires native_vlan or allowed_vlans")
        if len(set(allowed_vlans)) != len(allowed_vlans):
            raise ValueError("trunk port mode contains duplicate allowed_vlans")
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


def _admin_state_text(value) -> str:
    return admin_state_to_text(value)


def _validate_token(value: str, field: str) -> None:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{field} is required")
    if not _PROFILE_TOKEN_RE.fullmatch(value):
        raise ValueError(f"{field} must be a stable token")


def _validate_namespace(value: str, field: str) -> None:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{field} is required")
    if value != value.strip() or any(char.isspace() for char in value):
        raise ValueError(f"{field} must not contain whitespace")
    if not value.startswith(_NAMESPACE_SCHEMES):
        raise ValueError(f"{field} must be an absolute XML namespace URI")
    if value.endswith(":") or "{" in value or "}" in value:
        raise ValueError(f"{field} must be a stable XML namespace URI")
