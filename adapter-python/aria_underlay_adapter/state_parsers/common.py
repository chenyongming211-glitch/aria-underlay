from __future__ import annotations

from xml.etree import ElementTree

from aria_underlay_adapter.errors import AdapterError


class FixtureStateParser:
    profile = None

    @property
    def production_ready(self) -> bool:
        return self.profile.production_ready

    @property
    def fixture_verified(self) -> bool:
        return getattr(self.profile, "fixture_verified", False)

    def parse_running(self, xml: str, scope=None) -> dict:
        try:
            root = ElementTree.fromstring(xml)
        except ElementTree.ParseError as exc:
            raise _parse_error(f"invalid XML: {exc}") from exc

        vlans = _parse_vlans(root)
        interfaces = _parse_interfaces(root)
        return {
            "vlans": _filter_vlans(vlans, scope),
            "interfaces": _filter_interfaces(interfaces, scope),
        }


def _parse_vlans(root) -> list[dict]:
    vlans = []
    seen = set()
    for vlan in _children(_first_child(root, "vlans"), "vlan"):
        vlan_id = _parse_vlan_id(_required_text(vlan, "vlan-id", "vlan/vlan-id"))
        if vlan_id in seen:
            raise _parse_error(f"duplicate VLAN {vlan_id}")
        seen.add(vlan_id)
        vlans.append(
            {
                "vlan_id": vlan_id,
                "name": _optional_text(vlan, "name"),
                "description": _optional_text(vlan, "description"),
            }
        )
    return vlans


def _parse_interfaces(root) -> list[dict]:
    interfaces = []
    seen = set()
    for interface in _children(_first_child(root, "interfaces"), "interface"):
        name = _required_text(interface, "name", "interface/name")
        if name in seen:
            raise _parse_error(f"duplicate interface {name}")
        seen.add(name)
        mode_node = _required_child(interface, "port-mode", f"interface {name}/port-mode")
        interfaces.append(
            {
                "name": name,
                "admin_state": _optional_text(interface, "admin-state"),
                "description": _optional_text(interface, "description"),
                "mode": _parse_port_mode(mode_node),
            }
        )
    return interfaces


def _parse_port_mode(mode_node) -> dict:
    raw_kind = _required_text(mode_node, "kind", "port-mode/kind")
    kind = raw_kind.lower()
    if kind == "access":
        return {
            "kind": "access",
            "access_vlan": _parse_vlan_id(
                _required_text(mode_node, "access-vlan", "port-mode/access-vlan")
            ),
            "native_vlan": None,
            "allowed_vlans": [],
        }
    if kind == "trunk":
        native_vlan = _optional_vlan_id(mode_node, "native-vlan")
        allowed_vlans = [
            _parse_vlan_id(_text(vlan_id))
            for vlan_id in _children(_first_child(mode_node, "allowed-vlans"), "vlan-id")
        ]
        if native_vlan is None and not allowed_vlans:
            raise _parse_error("trunk port mode has no native or allowed VLAN")
        if len(set(allowed_vlans)) != len(allowed_vlans):
            raise _parse_error("trunk port mode has duplicate allowed VLAN")
        return {
            "kind": "trunk",
            "access_vlan": None,
            "native_vlan": native_vlan,
            "allowed_vlans": allowed_vlans,
        }
    raise _parse_error(f"unknown port mode {raw_kind}")


def _filter_vlans(vlans: list[dict], scope) -> list[dict]:
    if scope is None or getattr(scope, "full", False):
        return vlans
    scoped_ids = {int(vlan_id) for vlan_id in getattr(scope, "vlan_ids", [])}
    if not scoped_ids:
        return vlans
    return [vlan for vlan in vlans if vlan["vlan_id"] in scoped_ids]


def _filter_interfaces(interfaces: list[dict], scope) -> list[dict]:
    if scope is None or getattr(scope, "full", False):
        return interfaces
    scoped_names = {str(name) for name in getattr(scope, "interface_names", [])}
    if not scoped_names:
        return interfaces
    return [interface for interface in interfaces if interface["name"] in scoped_names]


def _first_child(parent, tag: str):
    if parent is None:
        return None
    for child in list(parent):
        if _local_name(child.tag) == tag:
            return child
    return None


def _required_child(parent, tag: str, path: str):
    child = _first_child(parent, tag)
    if child is None:
        raise _parse_error(f"missing required element: {path}")
    return child


def _children(parent, tag: str):
    if parent is None:
        return []
    return [child for child in list(parent) if _local_name(child.tag) == tag]


def _required_text(parent, tag: str, path: str) -> str:
    child = _first_child(parent, tag)
    if child is None or not _text(child):
        raise _parse_error(f"missing required text: {path}")
    return _text(child)


def _optional_text(parent, tag: str):
    child = _first_child(parent, tag)
    if child is None:
        return None
    value = _text(child)
    return value if value else None


def _optional_vlan_id(parent, tag: str):
    value = _optional_text(parent, tag)
    if value is None:
        return None
    return _parse_vlan_id(value)


def _parse_vlan_id(value: str) -> int:
    try:
        vlan_id = int(value)
    except ValueError as exc:
        raise _parse_error(f"invalid VLAN ID {value}") from exc
    if vlan_id < 1 or vlan_id > 4094:
        raise _parse_error(f"invalid VLAN ID {vlan_id}")
    return vlan_id


def _text(node) -> str:
    return (node.text or "").strip()


def _local_name(tag: str) -> str:
    if "}" in tag:
        return tag.rsplit("}", 1)[1]
    return tag


def _parse_error(summary: str) -> AdapterError:
    return AdapterError(
        code="NETCONF_STATE_PARSE_FAILED",
        message="NETCONF running state parser failed",
        normalized_error="state parse failed",
        raw_error_summary=summary,
        retryable=False,
    )
