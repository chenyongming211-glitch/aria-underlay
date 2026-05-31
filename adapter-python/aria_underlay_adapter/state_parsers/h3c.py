from __future__ import annotations

import re
from xml.etree import ElementTree

from aria_underlay_adapter.errors import AdapterError
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
        acl_node = _first_descendant(root, "ACL")
        high_risk_audit = _parse_high_risk_audit(root)
        if vlan_node is None and acl_node is None:
            return _attach_high_risk_audit(
                super().parse_running(xml, scope=scope),
                high_risk_audit,
            )

        vlans = _parse_real_vlans(vlan_node) if vlan_node is not None else []
        interfaces = (
            _parse_real_interfaces(
                root,
                vlan_node,
                model_hint=self._model_hint,
                scope=scope,
            )
            if vlan_node is not None
            else []
        )
        acls = _parse_real_acls(acl_node) if acl_node is not None else []
        acl_bindings = (
            _parse_real_acl_bindings(
                acl_node,
                model_hint=self._model_hint,
                scope=scope,
            )
            if acl_node is not None
            else []
        )
        return _attach_high_risk_audit({
            "vlans": _filter_vlans(vlans, scope),
            "interfaces": _filter_interfaces(interfaces, scope),
            "acls": _filter_acls(acls, scope),
            "acl_bindings": _filter_acl_bindings(acl_bindings, scope),
        }, high_risk_audit)


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


def _parse_real_acls(acl_node) -> list[dict]:
    acl_by_id = {}
    for group in _children(_first_child(acl_node, "Groups"), "Group"):
        group_type = _optional_text(group, "GroupType")
        if group_type != "1":
            continue
        acl_id = _parse_acl_id(_required_text(group, "GroupID", "ACL/Groups/Group/GroupID"))
        if acl_id in acl_by_id:
            raise _parse_error(f"duplicate ACL {acl_id}")
        acl_by_id[acl_id] = _acl_base(acl_id)
        acl_by_id[acl_id].update({
            "acl_id": acl_id,
            "name": None,
            "description": _optional_text(group, "Description"),
            "rules": [],
        })

    seen_rules = set()
    for rule in _children(_first_child(acl_node, "IPv4BasicRules"), "Rule"):
        acl_id = _parse_acl_id(_required_text(rule, "GroupID", "ACL/IPv4BasicRules/Rule/GroupID"))
        _validate_acl_id_for_kind(acl_id, "basic_ipv4")
        if acl_id not in acl_by_id:
            acl_by_id[acl_id] = _acl_base(acl_id)
        parsed_rule = _parse_acl_rule(rule, acl_id, kind="basic_ipv4")
        key = (acl_id, parsed_rule["sequence"])
        if key in seen_rules:
            raise _parse_error(f"duplicate ACL {acl_id} rule {parsed_rule['sequence']}")
        seen_rules.add(key)
        acl_by_id[acl_id]["rules"].append(parsed_rule)

    for rule in _children(_first_child(acl_node, "IPv4AdvanceRules"), "Rule"):
        acl_id = _parse_acl_id(_required_text(rule, "GroupID", "ACL/IPv4AdvanceRules/Rule/GroupID"))
        _validate_acl_id_for_kind(acl_id, "advanced_ipv4")
        if acl_id not in acl_by_id:
            acl_by_id[acl_id] = _acl_base(acl_id, kind="advanced_ipv4")
        parsed_rule = _parse_acl_rule(rule, acl_id, kind="advanced_ipv4")
        key = (acl_id, parsed_rule["sequence"])
        if key in seen_rules:
            raise _parse_error(f"duplicate ACL {acl_id} rule {parsed_rule['sequence']}")
        seen_rules.add(key)
        acl_by_id[acl_id]["rules"].append(parsed_rule)

    for acl in acl_by_id.values():
        acl["rules"].sort(key=lambda item: item["sequence"])
    return [acl_by_id[acl_id] for acl_id in sorted(acl_by_id)]


def _acl_base(acl_id: int, *, kind: str | None = None) -> dict:
    kind = kind or _acl_kind_from_id(acl_id)
    acl = {
        "acl_id": acl_id,
        "name": None,
        "description": None,
        "rules": [],
    }
    if kind == "basic_ipv4":
        acl["kind"] = kind
    return acl


def _parse_acl_rule(rule, acl_id: int, *, kind: str = "advanced_ipv4") -> dict:
    protocol = "ip"
    if kind == "advanced_ipv4":
        protocol = _parse_acl_protocol(
            _required_text(rule, "ProtocolType", f"ACL {acl_id}/ProtocolType")
        )
    return {
        "sequence": _parse_rule_sequence(
            _required_text(rule, "RuleID", f"ACL {acl_id}/RuleID")
        ),
        "action": _parse_acl_action(_required_text(rule, "Action", f"ACL {acl_id}/Action")),
        "protocol": protocol,
        "source": _parse_acl_endpoint(rule, "Src"),
        "destination": None if kind == "basic_ipv4" else _parse_acl_endpoint(rule, "Dst"),
        "source_port_eq": None if kind == "basic_ipv4" else _parse_acl_port(rule, "Src"),
        "destination_port_eq": None if kind == "basic_ipv4" else _parse_acl_port(rule, "Dst"),
        "description": _optional_text(rule, "Description"),
    }


def _parse_real_acl_bindings(acl_node, *, model_hint: str, scope) -> list[dict]:
    bindings = []
    seen = set()
    scope_names = _scope_names_by_ifindex(scope)
    for binding in _children(_first_child(acl_node, "PfilterApply"), "Pfilter"):
        if _optional_text(binding, "AppObjType") != "1":
            continue
        if _optional_text(binding, "AppAclType") != "1":
            continue
        raw_acl_id = _required_text(binding, "AppAclGroup", "ACL/Pfilter/AppAclGroup")
        if raw_acl_id == "0":
            continue
        acl_id = _parse_acl_id(raw_acl_id)
        ifindex = _parse_ifindex_from_text(
            _required_text(binding, "AppObjIndex", "ACL/Pfilter/AppObjIndex"),
            "ACL/Pfilter/AppObjIndex",
        )
        if not _scope_includes_acl_binding(scope, scope_names, ifindex, acl_id):
            continue
        try:
            interface_name = _interface_name(
                ifindex,
                model_hint=model_hint,
                scope_names=scope_names,
            )
        except AdapterError:
            continue
        direction = _parse_acl_direction(
            _required_text(binding, "AppDirection", "ACL/Pfilter/AppDirection")
        )
        key = (interface_name, direction)
        if key in seen:
            raise _parse_error(f"duplicate ACL binding {interface_name} {direction}")
        seen.add(key)
        bindings.append(
            {
                "interface_name": interface_name,
                "direction": direction,
                "acl_id": acl_id,
            }
        )
    return sorted(bindings, key=lambda item: (item["interface_name"], item["direction"]))


def _parse_acl_endpoint(rule, prefix: str) -> dict | None:
    any_value = _optional_text(rule, f"{prefix}Any")
    node = _first_child(rule, f"{prefix}IPv4")
    if node is None:
        return None
    if any_value is not None and any_value.lower() != "false":
        return None
    address = _required_text(node, f"{prefix}IPv4Addr", f"ACL rule/{prefix}IPv4Addr")
    wildcard = _required_text(node, f"{prefix}IPv4Wildcard", f"ACL rule/{prefix}IPv4Wildcard")
    return {
        "address": address,
        "wildcard": wildcard,
    }


def _parse_acl_port(rule, prefix: str) -> int | None:
    node = _first_child(rule, f"{prefix}Port")
    if node is None:
        return None
    op = _required_text(node, f"{prefix}PortOp", f"ACL rule/{prefix}PortOp")
    if op != "2":
        raise _parse_error(f"unsupported ACL {prefix.lower()} port operator {op}")
    value = _required_text(node, f"{prefix}PortValue1", f"ACL rule/{prefix}PortValue1")
    try:
        port = int(value)
    except ValueError as exc:
        raise _parse_error(f"invalid ACL port {value}") from exc
    if port < 1 or port > 65535:
        raise _parse_error(f"invalid ACL port {port}")
    return port


def _parse_ifindex(interface) -> int:
    value = _required_text(interface, "IfIndex", "interface/IfIndex")
    return _parse_ifindex_from_text(value, "interface/IfIndex")


def _parse_ifindex_from_text(value: str, path: str) -> int:
    try:
        ifindex = int(value)
    except ValueError as exc:
        raise _parse_error(f"invalid {path} {value}") from exc
    if ifindex <= 0:
        raise _parse_error(f"invalid {path} {ifindex}")
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


def _parse_acl_id(value: str) -> int:
    try:
        acl_id = int(value)
    except ValueError as exc:
        raise _parse_error(f"invalid ACL ID {value}") from exc
    if acl_id < 2000 or acl_id > 3999:
        raise _parse_error(f"invalid numeric IPv4 ACL ID {acl_id}")
    return acl_id


def _acl_kind_from_id(acl_id: int) -> str:
    if 2000 <= acl_id <= 2999:
        return "basic_ipv4"
    return "advanced_ipv4"


def _validate_acl_id_for_kind(acl_id: int, kind: str) -> None:
    if kind == "basic_ipv4" and not 2000 <= acl_id <= 2999:
        raise _parse_error(f"invalid IPv4 basic ACL ID {acl_id}")
    if kind == "advanced_ipv4" and not 3000 <= acl_id <= 3999:
        raise _parse_error(f"invalid IPv4 advanced ACL ID {acl_id}")


def _parse_rule_sequence(value: str) -> int:
    try:
        sequence = int(value)
    except ValueError as exc:
        raise _parse_error(f"invalid ACL rule sequence {value}") from exc
    if sequence < 0 or sequence > 65535:
        raise _parse_error(f"invalid ACL rule sequence {sequence}")
    return sequence


def _parse_acl_action(value: str) -> str:
    if value == "1":
        return "deny"
    if value == "2":
        return "permit"
    raise _parse_error(f"unsupported ACL action {value}")


def _parse_acl_protocol(value: str) -> str:
    if value == "256":
        return "ip"
    if value == "6":
        return "tcp"
    if value == "17":
        return "udp"
    if value == "1":
        return "icmp"
    raise _parse_error(f"unsupported ACL protocol {value}")


def _parse_acl_direction(value: str) -> str:
    if value == "1":
        return "inbound"
    if value == "2":
        return "outbound"
    raise _parse_error(f"unsupported ACL binding direction {value}")


def _attach_high_risk_audit(state: dict, audit: dict) -> dict:
    if not audit["features_present"]:
        return state
    enriched = dict(state)
    enriched["high_risk_audit"] = audit
    return enriched


def _parse_high_risk_audit(root) -> dict:
    pbr_nodes = _feature_nodes(root, "pbr")
    bgp_nodes = _feature_nodes(root, "bgp")
    pbr = _pbr_audit(pbr_nodes)
    bgp = _bgp_audit(bgp_nodes)

    features_present = []
    warnings = []
    if bgp["present"]:
        features_present.append("bgp")
        warnings.append(
            "BGP config detected; read-only audit only until path-level write evidence exists"
        )
    if pbr["present"]:
        features_present.append("pbr")
        warnings.append(
            "PBR config detected; read-only audit only until path-level write evidence exists"
        )

    return {
        "features_present": features_present,
        "write_decision": "read_only" if features_present else "allowed_vendor_private",
        "touched_scope": _high_risk_touched_scope(pbr, bgp),
        "pbr": pbr,
        "bgp": bgp,
        "warnings": warnings,
    }


def _pbr_audit(nodes: list[tuple[str, object]]) -> dict:
    return {
        "present": bool(nodes),
        "blast_radius": "policy_reference",
        "policies": _dedupe_sorted(
            _text_values(nodes, lambda name: "policy" in name and "route" not in name)
        ),
        "acl_references": _dedupe_sorted_ints(
            _integer_values(nodes, lambda name: "acl" in name)
        ),
        "interfaces": _dedupe_sorted(
            _text_values(nodes, lambda name: "interface" in name or name in {"ifname"})
        ),
        "raw_paths": [path for path, _node in nodes],
    }


def _bgp_audit(nodes: list[tuple[str, object]]) -> dict:
    as_numbers = _dedupe_sorted_ints(
        _integer_values(nodes, lambda name: name in {"as", "asnumber", "localas"})
    )
    neighbor_details = _bgp_neighbor_details(nodes)
    neighbor_addresses = [detail["address"] for detail in neighbor_details]
    neighbor_policies = [
        policy
        for detail in neighbor_details
        for policy in (detail["import_policy"], detail["export_policy"])
        if policy
    ]
    session_states = [
        detail["session_state"] for detail in neighbor_details if detail["session_state"]
    ]
    vrfs = [
        detail["vrf"] for detail in neighbor_details if detail["vrf"]
    ] + _text_values(nodes, lambda name: "vrf" in name or "vpninstance" in name)

    return {
        "present": bool(nodes),
        "blast_radius": "routing_control_plane",
        "as_numbers": as_numbers,
        "local_as": _single_value_or_none(as_numbers),
        "vrfs": _dedupe_sorted(vrfs),
        "neighbors": _dedupe_sorted(
            neighbor_addresses
            + _text_values(
                nodes,
                lambda name: ("peer" in name or "neighbor" in name),
                value_filter=_looks_like_ipv4,
            )
        ),
        "neighbor_details": neighbor_details,
        "session_states": _dedupe_sorted(session_states),
        "policy_references": _dedupe_sorted(
            neighbor_policies
            + _text_values(
                nodes,
                lambda name: "policy" in name,
                value_filter=lambda value: bool(value) and not value.isdigit(),
            )
        ),
        "raw_paths": [path for path, _node in nodes],
    }


def _bgp_neighbor_details(nodes: list[tuple[str, object]]) -> list[dict]:
    details = []
    seen = set()
    for bgp_path, bgp_node in nodes:
        for instance_path, instance in _bgp_instance_nodes(bgp_path, bgp_node):
            instance_vrf = _first_text_value(
                instance, lambda name: "vrf" in name or "vpninstance" in name
            )
            for peer_path, peer in _bgp_peer_nodes(instance_path, instance):
                address = _first_text_value(
                    peer, _bgp_neighbor_address_name, value_filter=_looks_like_ipv4
                )
                if address is None:
                    address = _first_ipv4_text(peer)
                if address is None:
                    continue

                detail = {
                    "address": address,
                    "remote_as": _first_integer_value(peer, _bgp_remote_as_name),
                    "session_state": _normalize_bgp_session_state(
                        _first_text_value(peer, _bgp_session_state_name)
                    ),
                    "import_policy": _first_text_value(peer, _bgp_import_policy_name),
                    "export_policy": _first_text_value(peer, _bgp_export_policy_name),
                    "vrf": _first_text_value(
                        peer, lambda name: "vrf" in name or "vpninstance" in name
                    )
                    or instance_vrf,
                    "raw_path": peer_path,
                }
                key = (
                    detail["vrf"],
                    detail["address"],
                    detail["remote_as"],
                    detail["raw_path"],
                )
                if key in seen:
                    continue
                seen.add(key)
                details.append(detail)

    return sorted(
        details,
        key=lambda detail: (
            detail["vrf"] or "",
            detail["address"],
            detail["remote_as"] or -1,
            detail["raw_path"],
        ),
    )


def _bgp_instance_nodes(bgp_path: str, bgp_node) -> list[tuple[str, object]]:
    instances = [
        (path, node)
        for path, node in _walk_paths_from(bgp_path, bgp_node)
        if _normalized_local_name(node.tag) == "instance"
    ]
    return instances or [(bgp_path, bgp_node)]


def _bgp_peer_nodes(instance_path: str, instance) -> list[tuple[str, object]]:
    return [
        (path, node)
        for path, node in _walk_paths_from(instance_path, instance)
        if _normalized_local_name(node.tag) in {"peer", "neighbor"}
    ]


def _first_text_value(node, name_filter, *, value_filter=None) -> str | None:
    for child in node.iter():
        name = _normalized_local_name(child.tag)
        if not name_filter(name):
            continue
        value = _text(child)
        if not value:
            continue
        if value_filter is not None and not value_filter(value):
            continue
        return value
    return None


def _first_integer_value(node, name_filter) -> int | None:
    value = _first_text_value(node, name_filter)
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def _first_ipv4_text(node) -> str | None:
    for child in node.iter():
        value = _text(child)
        if _looks_like_ipv4(value):
            return value
    return None


def _bgp_neighbor_address_name(name: str) -> bool:
    return (
        name
        in {
            "peeraddress",
            "peeraddr",
            "neighboraddress",
            "neighboraddr",
            "neighborip",
            "peerip",
        }
        or ("address" in name and ("peer" in name or "neighbor" in name))
    )


def _bgp_remote_as_name(name: str) -> bool:
    return name in {
        "peeras",
        "remoteas",
        "remoteasnumber",
        "peerremoteas",
        "neighboras",
        "neighborasnumber",
    } or ("as" in name and ("peer" in name or "remote" in name or "neighbor" in name))


def _bgp_session_state_name(name: str) -> bool:
    return name in {"state", "sessionstate", "peerstate", "neighborstate"} or (
        "state" in name and ("session" in name or "peer" in name or "neighbor" in name)
    )


def _bgp_import_policy_name(name: str) -> bool:
    return ("policy" in name and ("import" in name or "inbound" in name)) or name in {
        "inpolicy",
        "inroutepolicy",
    }


def _bgp_export_policy_name(name: str) -> bool:
    return ("policy" in name and ("export" in name or "outbound" in name)) or name in {
        "outpolicy",
        "outroutepolicy",
    }


def _normalize_bgp_session_state(value: str | None) -> str | None:
    if value is None:
        return None
    normalized = value.strip().lower()
    return normalized or None


def _single_value_or_none(values: list[int]) -> int | None:
    return values[0] if len(values) == 1 else None


def _walk_paths_from(base_path: str, root):
    def walk(node, path):
        yield (path, node)
        for child in list(node):
            child_path = f"{path}/{_local_name(child.tag)}"
            yield from walk(child, child_path)

    yield from walk(root, base_path)


def _high_risk_touched_scope(pbr: dict, bgp: dict) -> dict:
    return {
        "affected_vrfs": bgp["vrfs"],
        "bgp_as_numbers": bgp["as_numbers"],
        "bgp_neighbors": bgp["neighbors"],
        "route_policy_refs": bgp["policy_references"],
        "pbr_policy_refs": pbr["policies"],
        "acl_refs": pbr["acl_references"],
        "interfaces": pbr["interfaces"],
        "raw_paths": sorted(set(bgp["raw_paths"] + pbr["raw_paths"])),
    }


def _feature_nodes(root, feature: str) -> list[tuple[str, object]]:
    nodes = []
    for path, node in _walk_paths(root):
        name = _normalized_local_name(node.tag)
        if feature == "bgp" and "bgp" in name:
            nodes.append((path, node))
        elif feature == "pbr" and (
            "pbr" in name or ("policy" in name and "route" in name)
        ):
            nodes.append((path, node))
    return nodes


def _text_values(nodes, name_filter, *, value_filter=None) -> list[str]:
    values = []
    for _path, node in nodes:
        for child in node.iter():
            name = _normalized_local_name(child.tag)
            if not name_filter(name):
                continue
            value = _text(child)
            if not value:
                continue
            if value_filter is not None and not value_filter(value):
                continue
            values.append(value)
    return values


def _integer_values(nodes, name_filter) -> list[int]:
    values = []
    for value in _text_values(nodes, name_filter):
        try:
            values.append(int(value))
        except ValueError:
            continue
    return values


def _dedupe_sorted(values: list[str]) -> list[str]:
    return sorted({value for value in values if value})


def _dedupe_sorted_ints(values: list[int]) -> list[int]:
    return sorted(set(values))


def _looks_like_ipv4(value: str) -> bool:
    parts = value.split(".")
    if len(parts) != 4:
        return False
    try:
        return all(0 <= int(part) <= 255 for part in parts)
    except ValueError:
        return False


def _walk_paths(root):
    def walk(node, path):
        yield (path, node)
        for child in list(node):
            child_path = f"{path}/{_local_name(child.tag)}"
            yield from walk(child, child_path)

    yield from walk(root, f"/{_local_name(root.tag)}")


def _normalized_local_name(tag: str) -> str:
    return re.sub(r"[^a-z0-9]", "", _local_name(tag).lower())


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


def _scope_includes_acl_binding(
    scope,
    scope_names: dict[int, str],
    ifindex: int,
    acl_id: int,
) -> bool:
    if scope is None or getattr(scope, "full", False):
        return True
    interface_names = getattr(scope, "interface_names", [])
    acl_ids = getattr(scope, "acl_ids", [])
    if not interface_names and not acl_ids:
        return False
    scoped_acl_ids = set()
    for value in acl_ids:
        try:
            scoped_acl_ids.add(int(value))
        except (TypeError, ValueError) as exc:
            raise _parse_error(f"scope.acl_ids contains non-integer value {value!r}") from exc
    return ifindex in scope_names or acl_id in scoped_acl_ids


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


def _filter_acls(acls: list[dict], scope) -> list[dict]:
    if scope is None or getattr(scope, "full", False):
        return acls
    scoped_ids = set()
    for index, acl_id in enumerate(getattr(scope, "acl_ids", [])):
        try:
            scoped_ids.add(int(acl_id))
        except (TypeError, ValueError) as exc:
            raise _parse_error(
                f"scope.acl_ids[{index}] must be an integer: {acl_id!r}"
            ) from exc
    if not scoped_ids:
        return acls
    return [acl for acl in acls if acl["acl_id"] in scoped_ids]


def _filter_acl_bindings(bindings: list[dict], scope) -> list[dict]:
    if scope is None or getattr(scope, "full", False):
        return bindings
    scoped_interfaces = {
        _interface_alias_key(name)
        for name in getattr(scope, "interface_names", [])
    }
    scoped_acl_ids = set()
    for acl_id in getattr(scope, "acl_ids", []):
        try:
            scoped_acl_ids.add(int(acl_id))
        except (TypeError, ValueError) as exc:
            raise _parse_error(f"scope.acl_ids contains non-integer value {acl_id!r}") from exc
    if not scoped_interfaces and not scoped_acl_ids:
        return bindings
    return [
        binding
        for binding in bindings
        if _interface_alias_key(binding["interface_name"]) in scoped_interfaces
        or binding["acl_id"] in scoped_acl_ids
    ]


def _interface_alias_key(name: str) -> str:
    text = str(name).strip()
    for long_name, short_name in (
        ("GigabitEthernet", "GE"),
        ("Ten-GigabitEthernet", "XGE"),
        ("FortyGigE", "FGE"),
    ):
        if text.startswith(long_name):
            return f"{short_name}{text[len(long_name):]}"
    return text


def _first_descendant(parent, tag: str):
    for child in _descendants(parent, tag):
        return child
    return None


def _descendants(parent, tag: str):
    return [child for child in parent.iter() if _local_name(child.tag) == tag]
