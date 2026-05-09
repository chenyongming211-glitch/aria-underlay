from __future__ import annotations

from dataclasses import dataclass
import ipaddress
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

    @property
    def ACL_NAMESPACE(self) -> str:
        return H3C_COMWARE_CONFIG_NAMESPACE

    def render_edit_config(self, desired_state) -> str:
        vlan_nodes = [
            self.render_vlan_create(vlan)
            for vlan in getattr(desired_state, "vlans", [])
        ]
        acl_nodes = [
            self.render_acl_group(acl)
            for acl in getattr(desired_state, "acls", [])
        ]
        acl_rule_nodes = [
            rule_node
            for acl in getattr(desired_state, "acls", [])
            for rule_node in self.render_acl_rules(acl)
        ]
        acl_binding_nodes = [
            self.render_acl_binding(binding)
            for binding in getattr(desired_state, "acl_bindings", [])
        ]
        ifmgr_nodes = []
        access_nodes = []
        trunk_nodes = []
        for interface in getattr(desired_state, "interfaces", []):
            kind = _mode_kind(_field(interface, "mode"))
            description_node = self.render_interface_description(interface)
            if description_node is not None:
                ifmgr_nodes.append(description_node)
            node = self.render_interface_update(interface)
            if kind == "access":
                access_nodes.append(node)
            elif kind == "trunk":
                trunk_nodes.append(node)

        top_children = []
        if ifmgr_nodes:
            top_children.append(
                XmlElement(
                    "Ifmgr",
                    namespace=self.IFACE_NAMESPACE,
                    children=[
                        XmlElement(
                            "Interfaces",
                            namespace=self.IFACE_NAMESPACE,
                            children=ifmgr_nodes,
                        )
                    ],
                )
            )

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
        if vlan_children:
            top_children.append(
                XmlElement(
                    "VLAN",
                    namespace=self.VLAN_NAMESPACE,
                    children=vlan_children,
                )
            )
        acl_children = []
        if acl_nodes:
            acl_children.append(
                XmlElement("Groups", namespace=self.ACL_NAMESPACE, children=acl_nodes)
            )
        if acl_rule_nodes:
            acl_children.append(
                XmlElement(
                    "IPv4AdvanceRules",
                    namespace=self.ACL_NAMESPACE,
                    children=acl_rule_nodes,
                )
            )
        if acl_binding_nodes:
            acl_children.append(
                XmlElement(
                    "PfilterApply",
                    namespace=self.ACL_NAMESPACE,
                    children=acl_binding_nodes,
                )
            )
        if acl_children:
            top_children.append(
                XmlElement("ACL", namespace=self.ACL_NAMESPACE, children=acl_children)
            )
        if not top_children:
            raise AdapterError(
                code="EMPTY_DESIRED_STATE",
                message="desired state contains no VLAN, interface, or ACL changes",
                normalized_error="empty desired state",
                raw_error_summary="renderer refused to produce an empty edit-config payload",
                retryable=False,
            )

        return render_xml(
            XmlElement(
                "config",
                namespace=NETCONF_BASE_NAMESPACE,
                children=[
                    XmlElement(
                        "top",
                        namespace=self.VLAN_NAMESPACE,
                        children=top_children,
                    )
                ],
            )
        )

    def render_vlan_create(self, vlan) -> XmlElement:
        vlan_id = _validate_vlan_id(_field(vlan, "vlan_id"), "vlan.vlan_id")

        children = [XmlElement("ID", namespace=self.VLAN_NAMESPACE, children=[str(vlan_id)])]
        name = _optional_text(vlan, "name")
        if name is not None:
            children.append(XmlElement("Name", namespace=self.VLAN_NAMESPACE, children=[name]))
        description = _optional_text(vlan, "description")
        if description is not None:
            children.append(
                XmlElement("Description", namespace=self.VLAN_NAMESPACE, children=[description])
            )
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

    def render_interface_description(self, interface) -> XmlElement | None:
        description = _optional_text(interface, "description")
        if description is None:
            return None
        name = _required_text(interface, "name")
        ifindex = _interface_ifindex(name)
        return XmlElement(
            "Interface",
            namespace=self.IFACE_NAMESPACE,
            children=[
                XmlElement("IfIndex", namespace=self.IFACE_NAMESPACE, children=[str(ifindex)]),
                XmlElement(
                    "Description",
                    namespace=self.IFACE_NAMESPACE,
                    children=[description],
                ),
            ],
        )

    def render_acl_group(self, acl) -> XmlElement:
        acl_id = _validate_acl_id(_field(acl, "acl_id"), "acl.acl_id")
        if _optional_text(acl, "name") is not None:
            raise ValueError("H3C numeric IPv4 advanced ACL name is not supported")
        children = [
            XmlElement("GroupType", namespace=self.ACL_NAMESPACE, children=["1"]),
            XmlElement("GroupID", namespace=self.ACL_NAMESPACE, children=[str(acl_id)]),
        ]
        description = _optional_text(acl, "description")
        if description is not None:
            children.append(
                XmlElement("Description", namespace=self.ACL_NAMESPACE, children=[description])
            )
        return XmlElement("Group", namespace=self.ACL_NAMESPACE, children=children)

    def render_acl_rules(self, acl) -> list[XmlElement]:
        acl_id = _validate_acl_id(_field(acl, "acl_id"), "acl.acl_id")
        rules = []
        seen = set()
        for rule in _repeated_field(acl, "rules"):
            sequence = _validate_rule_sequence(_field(rule, "sequence"))
            if sequence in seen:
                raise ValueError(f"duplicate ACL rule sequence {sequence}")
            seen.add(sequence)
            rules.append(self.render_acl_rule(acl_id, rule))
        return rules

    def render_acl_rule(self, acl_id: int, rule) -> XmlElement:
        action = _acl_action(_field(rule, "action"))
        protocol = _acl_protocol(_field(rule, "protocol"))
        children = [
            XmlElement("GroupID", namespace=self.ACL_NAMESPACE, children=[str(acl_id)]),
            XmlElement(
                "RuleID",
                namespace=self.ACL_NAMESPACE,
                children=[str(_validate_rule_sequence(_field(rule, "sequence")))],
            ),
            XmlElement(
                "Action",
                namespace=self.ACL_NAMESPACE,
                children=[str(_h3c_acl_action_code(action))],
            ),
            XmlElement(
                "ProtocolType",
                namespace=self.ACL_NAMESPACE,
                children=[str(_h3c_acl_protocol_code(protocol))],
            ),
        ]
        description = _optional_text(rule, "description")
        if description is not None:
            children.append(
                XmlElement("Description", namespace=self.ACL_NAMESPACE, children=[description])
            )
        source = _optional_field(rule, "source")
        if source is not None:
            children.extend(_acl_endpoint_nodes("Src", source, self.ACL_NAMESPACE))
        destination = _optional_field(rule, "destination")
        if destination is not None:
            children.extend(_acl_endpoint_nodes("Dst", destination, self.ACL_NAMESPACE))

        source_port = _optional_field(rule, "source_port_eq")
        if source_port is not None:
            children.append(_acl_port_node("Src", source_port, protocol, self.ACL_NAMESPACE))
        destination_port = _optional_field(rule, "destination_port_eq")
        if destination_port is not None:
            children.append(_acl_port_node("Dst", destination_port, protocol, self.ACL_NAMESPACE))
        return XmlElement("Rule", namespace=self.ACL_NAMESPACE, children=children)

    def render_acl_binding(self, binding) -> XmlElement:
        interface_name = _required_text(binding, "interface_name")
        return XmlElement(
            "Pfilter",
            namespace=self.ACL_NAMESPACE,
            children=[
                XmlElement("AppObjType", namespace=self.ACL_NAMESPACE, children=["1"]),
                XmlElement(
                    "AppObjIndex",
                    namespace=self.ACL_NAMESPACE,
                    children=[str(_interface_ifindex(interface_name))],
                ),
                XmlElement(
                    "AppDirection",
                    namespace=self.ACL_NAMESPACE,
                    children=[str(_acl_direction_code(_field(binding, "direction")))],
                ),
                XmlElement("AppAclType", namespace=self.ACL_NAMESPACE, children=["1"]),
                XmlElement(
                    "AppAclGroup",
                    namespace=self.ACL_NAMESPACE,
                    children=[
                        str(_validate_acl_id(_field(binding, "acl_id"), "acl_binding.acl_id"))
                    ],
                ),
            ],
        )


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


def _validate_acl_id(value, field: str) -> int:
    if value is None:
        raise ValueError(f"{field} is required")
    try:
        acl_id = int(value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{field} must be an integer ACL ID") from exc
    if acl_id < 3000 or acl_id > 3999:
        raise ValueError(f"{field} must be in range 3000..3999")
    return acl_id


def _validate_rule_sequence(value) -> int:
    try:
        sequence = int(value)
    except (TypeError, ValueError) as exc:
        raise ValueError("ACL rule sequence must be an integer") from exc
    if sequence < 0 or sequence > 65535:
        raise ValueError("ACL rule sequence must be in range 0..65535")
    return sequence


def _validate_port(value, field: str) -> int:
    try:
        port = int(value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{field} must be an integer port") from exc
    if port < 1 or port > 65535:
        raise ValueError(f"{field} must be in range 1..65535")
    return port


def _acl_action(value) -> str:
    normalized = value.strip().lower() if isinstance(value, str) else value
    if normalized in {"permit", 1}:
        return "permit"
    if normalized in {"deny", 2}:
        return "deny"
    raise ValueError(f"unknown ACL action: {value}")


def _acl_protocol(value) -> str:
    normalized = value.strip().lower() if isinstance(value, str) else value
    if normalized in {"ip", 1}:
        return "ip"
    if normalized in {"tcp", 2}:
        return "tcp"
    if normalized in {"udp", 3}:
        return "udp"
    if normalized in {"icmp", 4}:
        return "icmp"
    raise ValueError(f"unknown ACL protocol: {value}")


def _acl_direction_code(value) -> int:
    normalized = value.strip().lower() if isinstance(value, str) else value
    if normalized in {"inbound", "in", 1}:
        return 1
    if normalized in {"outbound", "out", 2}:
        return 2
    raise ValueError(f"unknown ACL direction: {value}")


def _h3c_acl_action_code(action: str) -> int:
    return {"deny": 1, "permit": 2}[action]


def _h3c_acl_protocol_code(protocol: str) -> int:
    return {"icmp": 1, "tcp": 6, "udp": 17, "ip": 256}[protocol]


def _acl_endpoint_nodes(prefix: str, endpoint, namespace: str) -> list[XmlElement]:
    address = _required_text(endpoint, "address")
    wildcard = _required_text(endpoint, "wildcard")
    _validate_ipv4(address, f"{prefix}IPv4Addr")
    _validate_ipv4(wildcard, f"{prefix}IPv4Wildcard")
    return [
        XmlElement(f"{prefix}Any", namespace=namespace, children=["false"]),
        XmlElement(
            f"{prefix}IPv4",
            namespace=namespace,
            children=[
                XmlElement(f"{prefix}IPv4Addr", namespace=namespace, children=[address]),
                XmlElement(f"{prefix}IPv4Wildcard", namespace=namespace, children=[wildcard]),
            ],
        ),
    ]


def _acl_port_node(prefix: str, value, protocol: str, namespace: str) -> XmlElement:
    if protocol not in {"tcp", "udp"}:
        raise ValueError("ACL port matches require tcp or udp protocol")
    port = _validate_port(value, f"{prefix.lower()}_port_eq")
    return XmlElement(
        f"{prefix}Port",
        namespace=namespace,
        children=[
            XmlElement(f"{prefix}PortOp", namespace=namespace, children=["2"]),
            XmlElement(f"{prefix}PortValue1", namespace=namespace, children=[str(port)]),
            XmlElement(f"{prefix}PortValue2", namespace=namespace, children=["65536"]),
        ],
    )


def _validate_ipv4(value: str, field: str) -> None:
    try:
        ipaddress.IPv4Address(value)
    except ipaddress.AddressValueError as exc:
        raise ValueError(f"{field} must be an IPv4 address") from exc


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
