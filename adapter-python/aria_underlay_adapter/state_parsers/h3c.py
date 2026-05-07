from __future__ import annotations

import re
from xml.etree import ElementTree

from aria_underlay_adapter.state_parsers.common import (
    FixtureStateParser,
    _children,
    _filter_interfaces,
    _filter_vlans,
    _first_child,
    _local_name,
    _optional_text,
    _parse_error,
    _parse_vlan_id,
    _required_text,
    _text,
)
from aria_underlay_adapter.state_parsers.skeleton import StateParserProfile


H3C_COMWARE7_STATE_PARSER_PROFILE = StateParserProfile(
    vendor="h3c",
    profile_name="comware7-state-real",
    production_ready=True,
    fixture_verified=True,
)


class H3cStateParser(FixtureStateParser):
    """Running state parser for H3C Comware NETCONF XML."""

    profile = H3C_COMWARE7_STATE_PARSER_PROFILE

    def __init__(self, model_hint: str | None = None):
        self._model_hint = model_hint or ""

    def parse_running(self, xml: str, scope=None) -> dict:
        try:
            root = ElementTree.fromstring(xml)
        except ElementTree.ParseError as exc:
            raise _parse_error(f"invalid XML: {exc}") from exc

        vlan_node = _first_descendant(root, "VLAN")
        if vlan_node is None:
            return super().parse_running(xml, scope=scope)

        vlans = _parse_real_vlans(vlan_node)
        interfaces = _parse_real_interfaces(
            root,
            vlan_node,
            model_hint=self._model_hint,
            scope=scope,
        )
        return {
            "vlans": _filter_vlans(vlans, scope),
            "interfaces": _filter_interfaces(interfaces, scope),
        }


def _parse_real_vlans(vlan_node) -> list[dict]:
    vlans = []
    seen = set()
    for vlan in _children(_first_child(vlan_node, "VLANs"), "VLANID"):
        vlan_id = _parse_vlan_id(_required_text(vlan, "ID", "VLAN/VLANs/VLANID/ID"))
        if vlan_id in seen:
            raise _parse_error(f"duplicate VLAN {vlan_id}")
        seen.add(vlan_id)
        vlans.append(
            {
                "vlan_id": vlan_id,
                "name": _optional_text(vlan, "Name"),
                "description": _optional_text(vlan, "Description"),
            }
        )
    return vlans


def _parse_real_interfaces(root, vlan_node, *, model_hint: str, scope) -> list[dict]:
    descriptions = _descriptions_by_ifindex(root)
    scope_names = _scope_names_by_ifindex(scope)
    interfaces = []
    seen = set()

    for interface in _children(_first_child(vlan_node, "AccessInterfaces"), "Interface"):
        ifindex = _parse_ifindex(interface)
        if not _scope_includes_ifindex(scope, scope_names, ifindex):
            continue
        name = _interface_name(ifindex, model_hint=model_hint, scope_names=scope_names)
        if name in seen:
            raise _parse_error(f"duplicate interface {name}")
        seen.add(name)
        interfaces.append(
            {
                "name": name,
                "admin_state": None,
                "description": descriptions.get(ifindex),
                "mode": {
                    "kind": "access",
                    "access_vlan": _parse_vlan_id(
                        _required_text(interface, "PVID", f"interface {name}/PVID")
                    ),
                    "native_vlan": None,
                    "allowed_vlans": [],
                },
            }
        )

    for interface in _children(_first_child(vlan_node, "TrunkInterfaces"), "Interface"):
        ifindex = _parse_ifindex(interface)
        if not _scope_includes_ifindex(scope, scope_names, ifindex):
            continue
        name = _interface_name(ifindex, model_hint=model_hint, scope_names=scope_names)
        if name in seen:
            raise _parse_error(f"duplicate interface {name}")
        seen.add(name)
        allowed_vlans = _parse_vlan_list(
            _required_text(
                interface,
                "PermitVlanList",
                f"interface {name}/PermitVlanList",
            )
        )
        if not allowed_vlans:
            raise _parse_error("trunk port mode has no native or allowed VLAN")
        interfaces.append(
            {
                "name": name,
                "admin_state": None,
                "description": descriptions.get(ifindex),
                "mode": {
                    "kind": "trunk",
                    "access_vlan": None,
                    "native_vlan": None,
                    "allowed_vlans": allowed_vlans,
                },
            }
        )

    return interfaces


def _parse_ifindex(interface) -> int:
    value = _required_text(interface, "IfIndex", "interface/IfIndex")
    try:
        ifindex = int(value)
    except ValueError as exc:
        raise _parse_error(f"invalid IfIndex {value}") from exc
    if ifindex <= 0:
        raise _parse_error(f"invalid IfIndex {ifindex}")
    return ifindex


def _parse_vlan_list(value: str) -> list[int]:
    vlans = []
    for raw_part in value.split(","):
        part = raw_part.strip()
        if not part:
            continue
        if "-" in part:
            raw_start, raw_end = [item.strip() for item in part.split("-", 1)]
            start = _parse_vlan_id(raw_start)
            end = _parse_vlan_id(raw_end)
            if start > end:
                raise _parse_error(f"invalid VLAN range {part}")
            vlans.extend(range(start, end + 1))
        else:
            vlans.append(_parse_vlan_id(part))
    if len(set(vlans)) != len(vlans):
        raise _parse_error("trunk port mode has duplicate allowed VLAN")
    return vlans


def _descriptions_by_ifindex(root) -> dict[int, str]:
    descriptions = {}
    for interface in _descendants(root, "Interface"):
        ifindex_node = _first_child(interface, "IfIndex")
        description = _optional_text(interface, "Description")
        if ifindex_node is None or not description:
            continue
        try:
            ifindex = int(_text(ifindex_node))
        except ValueError:
            continue
        descriptions.setdefault(ifindex, description)
    return descriptions


def _scope_names_by_ifindex(scope) -> dict[int, str]:
    if scope is None:
        return {}
    names = {}
    for name in getattr(scope, "interface_names", []):
        text = str(name)
        match = re.search(r"/(\d+)(?:\.\d+)?$", text)
        if match:
            names[int(match.group(1))] = text
    return names


def _scope_includes_ifindex(scope, scope_names: dict[int, str], ifindex: int) -> bool:
    if scope is None or getattr(scope, "full", False):
        return True
    if not getattr(scope, "interface_names", []):
        return False
    return ifindex in scope_names


def _interface_name(ifindex: int, *, model_hint: str, scope_names: dict[int, str]) -> str:
    if ifindex in scope_names:
        return scope_names[ifindex]

    model = model_hint.upper()
    if "S6800" in model:
        if 1 <= ifindex <= 48:
            return f"Ten-GigabitEthernet1/0/{ifindex}"
        if 49 <= ifindex <= 54:
            return f"FortyGigE1/0/{ifindex}"
    if "S5560" in model:
        if 1 <= ifindex <= 48:
            return f"GigabitEthernet1/0/{ifindex}"
        if 49 <= ifindex <= 52:
            return f"Ten-GigabitEthernet1/0/{ifindex}"

    raise _parse_error(
        f"H3C interface IfIndex {ifindex} cannot be mapped without a supported model_hint"
    )


def _first_descendant(parent, tag: str):
    for child in _descendants(parent, tag):
        return child
    return None


def _descendants(parent, tag: str):
    return [child for child in parent.iter() if _local_name(child.tag) == tag]
