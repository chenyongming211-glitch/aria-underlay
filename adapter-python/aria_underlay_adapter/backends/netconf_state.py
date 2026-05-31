from __future__ import annotations

from xml.sax.saxutils import escape

from aria_underlay_adapter.backends.netconf_errors import adapter_operation_error
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.normalization import admin_state_to_text as _admin_state_to_text


H3C_COMWARE_CONFIG_NS = "http://www.h3c.com/netconf/config:1.0"


def build_state_filter(scope=None, *, parser=None):
    if scope is None or getattr(scope, "full", False):
        return None

    vlan_ids = _normalized_scope_vlan_ids(scope)
    interface_names = _normalized_scope_interface_names(scope)
    acl_ids = _normalized_scope_acl_ids(scope)
    if not vlan_ids and not interface_names and not acl_ids:
        return None

    if _parser_vendor(parser) == "h3c":
        return f'<top xmlns="{H3C_COMWARE_CONFIG_NS}"><Ifmgr/><VLAN/><ACL/></top>'

    parts = []
    if vlan_ids:
        parts.append("<vlans>")
        for vlan_id in vlan_ids:
            parts.append(f"<vlan><vlan-id>{vlan_id}</vlan-id></vlan>")
        parts.append("</vlans>")
    if interface_names:
        parts.append("<interfaces>")
        for name in interface_names:
            parts.append(f"<interface><name>{escape(name)}</name></interface>")
        parts.append("</interfaces>")
    if not parts:
        return None
    return "".join(parts)


def read_running_config(session, scope=None, parser=None):
    filter_xml = build_state_filter(scope, parser=parser)
    kwargs = {"source": "running"}
    if filter_xml is not None:
        kwargs["filter"] = ("subtree", filter_xml)

    try:
        return _running_xml_from_reply(session.get_config(**kwargs))
    except Exception as exc:
        raise adapter_operation_error(
            code="NETCONF_GET_CONFIG_FAILED",
            message="NETCONF get-config running failed",
            exc=exc,
            retryable=True,
        ) from exc


def read_candidate_config(session):
    try:
        return _running_xml_from_reply(session.get_config(source="candidate"))
    except Exception as exc:
        raise adapter_operation_error(
            code="NETCONF_GET_CANDIDATE_CONFIG_FAILED",
            message="NETCONF get-config candidate failed",
            exc=exc,
            retryable=True,
        ) from exc


def _running_xml_from_reply(reply) -> str:
    if isinstance(reply, str):
        return reply
    for attr in ("data_xml", "xml"):
        value = getattr(reply, attr, None)
        if value:
            return str(value)
    data = getattr(reply, "data", None)
    if data is not None:
        return str(data)
    return str(reply)


def _parser_vendor(parser) -> str | None:
    profile = getattr(parser, "profile", None)
    vendor = getattr(profile, "vendor", None)
    if vendor is None:
        return None
    return str(vendor).strip().lower() or None


def parse_running_state(
    parser,
    xml: str,
    scope=None,
    *,
    allow_fixture_verified: bool = False,
) -> dict:
    if parser is None:
        raise AdapterError(
            code="NETCONF_STATE_PARSE_NOT_IMPLEMENTED",
            message="real NETCONF running state parser is not configured",
            normalized_error="state parser missing",
            raw_error_summary=(
                "scoped get-config completed, but no production state parser is configured; "
                f"scope={scope_summary(scope)}"
            ),
            retryable=False,
        )
    if not getattr(parser, "production_ready", False) and not (
        allow_fixture_verified and getattr(parser, "fixture_verified", False)
    ):
        raise AdapterError(
            code="NETCONF_STATE_PARSER_NOT_PRODUCTION_READY",
            message="NETCONF running state parser is not production ready",
            normalized_error="state parser not production ready",
            raw_error_summary=(
                "parser exists but is still a skeleton or test parser; "
                "refusing to trust running state"
            ),
            retryable=False,
        )
    try:
        state = parser.parse_running(xml, scope=scope)
    except AdapterError:
        raise
    except Exception as exc:
        raise adapter_operation_error(
            code="NETCONF_STATE_PARSE_FAILED",
            message="NETCONF running state parser failed",
            exc=exc,
            retryable=False,
        ) from exc

    return _validate_observed_state_shape(state)


def _validate_observed_state_shape(state: dict) -> dict:
    if not isinstance(state, dict):
        raise AdapterError(
            code="NETCONF_STATE_PARSE_FAILED",
            message="NETCONF running state parser returned invalid state",
            normalized_error="invalid parsed state",
            raw_error_summary=f"parsed_state_type={type(state).__name__}",
            retryable=False,
        )
    state.setdefault("vlans", [])
    state.setdefault("interfaces", [])
    state.setdefault("acls", [])
    state.setdefault("acl_bindings", [])
    return state


def scope_is_empty(scope) -> bool:
    return (
        scope is not None
        and not getattr(scope, "full", False)
        and not list(getattr(scope, "vlan_ids", []))
        and not list(getattr(scope, "interface_names", []))
        and not list(getattr(scope, "acl_ids", []))
    )


def desired_state_is_empty(desired_state) -> bool:
    return (
        not list(getattr(desired_state, "vlans", []))
        and not list(getattr(desired_state, "interfaces", []))
        and not list(getattr(desired_state, "acls", []))
        and not list(getattr(desired_state, "acl_bindings", []))
        and not list(getattr(desired_state, "delete_vlan_ids", []))
        and not list(getattr(desired_state, "delete_acl_ids", []))
        and not list(getattr(desired_state, "delete_acl_bindings", []))
    )


def _normalized_scope_vlan_ids(scope) -> list[int]:
    normalized = set()
    for index, vlan_id in enumerate(getattr(scope, "vlan_ids", [])):
        try:
            normalized.add(int(vlan_id))
        except (TypeError, ValueError) as exc:
            raise AdapterError(
                code="INVALID_STATE_SCOPE",
                message="state scope contains non-integer VLAN IDs",
                normalized_error="invalid state scope",
                raw_error_summary=(
                    f"scope.vlan_ids[{index}] must be an integer: {vlan_id!r}"
                ),
                retryable=False,
            ) from exc

    vlan_ids = sorted(normalized)
    invalid = [vlan_id for vlan_id in vlan_ids if vlan_id < 1 or vlan_id > 4094]
    if invalid:
        raise AdapterError(
            code="INVALID_STATE_SCOPE",
            message="state scope contains invalid VLAN IDs",
            normalized_error="invalid state scope",
            raw_error_summary=f"invalid_vlan_ids={invalid}",
            retryable=False,
        )
    return vlan_ids


def _normalized_scope_interface_names(scope) -> list[str]:
    names = sorted({str(name) for name in getattr(scope, "interface_names", [])})
    invalid = [name for name in names if not name.strip()]
    if invalid:
        raise AdapterError(
            code="INVALID_STATE_SCOPE",
            message="state scope contains empty interface names",
            normalized_error="invalid state scope",
            raw_error_summary="empty interface name in StateScope.interface_names",
            retryable=False,
        )
    return names


def _normalized_scope_acl_ids(scope) -> list[int]:
    normalized = set()
    for index, acl_id in enumerate(getattr(scope, "acl_ids", [])):
        try:
            normalized.add(int(acl_id))
        except (TypeError, ValueError) as exc:
            raise AdapterError(
                code="INVALID_STATE_SCOPE",
                message="state scope contains non-integer ACL IDs",
                normalized_error="invalid state scope",
                raw_error_summary=(
                    f"scope.acl_ids[{index}] must be an integer: {acl_id!r}"
                ),
                retryable=False,
            ) from exc
    acl_ids = sorted(normalized)
    invalid = [acl_id for acl_id in acl_ids if acl_id < 2000 or acl_id > 3999]
    if invalid:
        raise AdapterError(
            code="INVALID_STATE_SCOPE",
            message="state scope contains invalid numeric IPv4 ACL IDs",
            normalized_error="invalid state scope",
            raw_error_summary=f"invalid_acl_ids={invalid}",
            retryable=False,
        )
    return acl_ids


def verify_vlans(desired_state, observed: dict, scope=None) -> None:
    observed_by_id = {vlan["vlan_id"]: vlan for vlan in observed["vlans"]}
    desired_by_id = {
        _field(vlan, "vlan_id"): vlan
        for vlan in getattr(desired_state, "vlans", [])
    }
    for vlan_id in _scoped_vlan_ids(scope, observed):
        if vlan_id not in desired_by_id and vlan_id in observed_by_id:
            raise _verify_mismatch(
                f"VLAN {vlan_id} should be absent but exists in observed scoped state",
            )
    for desired_vlan in _desired_vlans_in_scope(desired_state, scope):
        vlan_id = _field(desired_vlan, "vlan_id")
        observed_vlan = observed_by_id.get(vlan_id)
        if observed_vlan is None:
            raise _verify_mismatch(f"VLAN {vlan_id} missing from observed scoped state")
        expected_name = _optional_field(desired_vlan, "name")
        expected_description = _optional_field(desired_vlan, "description")
        if observed_vlan.get("name") != expected_name:
            raise _verify_mismatch(
                f"VLAN {vlan_id} name mismatch: expected {expected_name!r}, "
                f"got {observed_vlan.get('name')!r}",
            )
        if observed_vlan.get("description") != expected_description:
            raise _verify_mismatch(
                f"VLAN {vlan_id} description mismatch: expected "
                f"{expected_description!r}, got {observed_vlan.get('description')!r}",
            )


def verify_interfaces(desired_state, observed: dict, scope=None) -> None:
    observed_by_name = {
        _interface_alias_key(interface["name"]): interface
        for interface in observed["interfaces"]
    }
    desired_by_name = {
        _interface_alias_key(_field(interface, "name")): interface
        for interface in getattr(desired_state, "interfaces", [])
    }
    for name in _scoped_interface_names(scope, observed):
        key = _interface_alias_key(name)
        if key not in desired_by_name and key in observed_by_name:
            raise _verify_mismatch(
                f"interface {name} should be absent but exists in observed scoped state",
            )
    for desired_interface in _desired_interfaces_in_scope(desired_state, scope):
        name = _field(desired_interface, "name")
        observed_interface = observed_by_name.get(_interface_alias_key(name))
        if observed_interface is None:
            raise _verify_mismatch(
                f"interface {name} missing from observed scoped state",
            )
        expected_admin_state = _admin_state_to_text(_field(desired_interface, "admin_state"))
        expected_description = _optional_field(desired_interface, "description")
        expected_mode = _mode_to_dict(_field(desired_interface, "mode"))

        observed_admin_state = observed_interface.get("admin_state")
        if (
            observed_admin_state is not None
            and observed_admin_state != expected_admin_state
        ):
            raise _verify_mismatch(
                f"interface {name} admin state mismatch: expected "
                f"{expected_admin_state!r}, got {observed_admin_state!r}",
            )
        if observed_interface.get("description") != expected_description:
            raise _verify_mismatch(
                f"interface {name} description mismatch: expected "
                f"{expected_description!r}, got {observed_interface.get('description')!r}",
            )
        if _normalize_mode(observed_interface.get("mode")) != _normalize_mode(expected_mode):
            raise _verify_mismatch(
                f"interface {name} mode mismatch: expected {expected_mode!r}, "
                f"got {observed_interface.get('mode')!r}",
            )


def verify_acls(desired_state, observed: dict, scope=None) -> None:
    observed_by_id = {acl["acl_id"]: acl for acl in observed["acls"]}
    desired_by_id = {
        _field(acl, "acl_id"): acl
        for acl in getattr(desired_state, "acls", [])
    }
    for acl_id in _scoped_acl_ids(scope, observed):
        if acl_id not in desired_by_id and acl_id in observed_by_id:
            raise _verify_mismatch(
                f"ACL {acl_id} should be absent but exists in observed scoped state",
            )
    for desired_acl in _desired_acls_in_scope(desired_state, scope):
        acl_id = _field(desired_acl, "acl_id")
        observed_acl = observed_by_id.get(acl_id)
        if observed_acl is None:
            raise _verify_mismatch(f"ACL {acl_id} missing from observed scoped state")
        expected_name = _optional_field(desired_acl, "name")
        expected_description = _optional_field(desired_acl, "description")
        if observed_acl.get("name") != expected_name:
            raise _verify_mismatch(
                f"ACL {acl_id} name mismatch: expected {expected_name!r}, "
                f"got {observed_acl.get('name')!r}",
            )
        if observed_acl.get("description") != expected_description:
            raise _verify_mismatch(
                f"ACL {acl_id} description mismatch: expected "
                f"{expected_description!r}, got {observed_acl.get('description')!r}",
            )
        expected_kind = _acl_kind_text(_optional_field(desired_acl, "kind"), acl_id)
        observed_kind = _acl_kind_text(observed_acl.get("kind"), acl_id)
        if observed_kind != expected_kind:
            raise _verify_mismatch(
                f"ACL {acl_id} kind mismatch: expected {expected_kind!r}, "
                f"got {observed_kind!r}",
            )
        if _normalize_acl_rules(observed_acl.get("rules", [])) != _normalize_acl_rules(
            _repeated_field(desired_acl, "rules")
        ):
            raise _verify_mismatch(
                f"ACL {acl_id} rules mismatch: expected "
                f"{_normalize_acl_rules(_repeated_field(desired_acl, 'rules'))!r}, "
                f"got {_normalize_acl_rules(observed_acl.get('rules', []))!r}",
            )


def verify_acl_bindings(desired_state, observed: dict, scope=None) -> None:
    observed_by_key = {
        _acl_binding_key(binding["interface_name"], binding["direction"]): binding
        for binding in observed["acl_bindings"]
    }
    for delete_binding in getattr(desired_state, "delete_acl_bindings", []):
        interface_name = _field(delete_binding, "interface_name")
        direction = _acl_direction_text(_field(delete_binding, "direction"))
        key = _acl_binding_key(interface_name, direction)
        observed_binding = observed_by_key.get(key)
        if observed_binding is not None and observed_binding.get("acl_id") == _field(
            delete_binding,
            "acl_id",
        ):
            raise _verify_mismatch(
                f"ACL binding {interface_name} {direction} should be absent but exists"
            )
    for desired_binding in _desired_acl_bindings_in_scope(desired_state, scope):
        interface_name = _field(desired_binding, "interface_name")
        direction = _acl_direction_text(_field(desired_binding, "direction"))
        key = _acl_binding_key(interface_name, direction)
        observed_binding = observed_by_key.get(key)
        if observed_binding is None:
            raise _verify_mismatch(
                f"ACL binding {interface_name} {direction} missing from observed scoped state"
            )
        expected_acl_id = _field(desired_binding, "acl_id")
        if observed_binding.get("acl_id") != expected_acl_id:
            raise _verify_mismatch(
                f"ACL binding {interface_name} {direction} mismatch: expected "
                f"ACL {expected_acl_id}, got ACL {observed_binding.get('acl_id')}"
            )


def _desired_vlans_in_scope(desired_state, scope=None):
    if scope is not None and not getattr(scope, "full", False) and not getattr(scope, "vlan_ids", []):
        return
    vlan_ids = set(getattr(scope, "vlan_ids", [])) if scope is not None else set()
    for vlan in getattr(desired_state, "vlans", []):
        if getattr(scope, "full", False) or scope is None or _field(vlan, "vlan_id") in vlan_ids:
            yield vlan


def _scoped_vlan_ids(scope=None, observed=None):
    if scope is None or getattr(scope, "full", False):
        if observed is None:
            return []
        return {vlan["vlan_id"] for vlan in observed["vlans"]}
    return set(getattr(scope, "vlan_ids", []))


def _desired_interfaces_in_scope(desired_state, scope=None):
    if (
        scope is not None
        and not getattr(scope, "full", False)
        and not getattr(scope, "interface_names", [])
    ):
        return
    interface_names = (
        {_interface_alias_key(name) for name in getattr(scope, "interface_names", [])}
        if scope is not None
        else set()
    )
    for interface in getattr(desired_state, "interfaces", []):
        if (
            getattr(scope, "full", False)
            or scope is None
            or _interface_alias_key(_field(interface, "name")) in interface_names
        ):
            yield interface


def _scoped_interface_names(scope=None, observed=None):
    if scope is None or getattr(scope, "full", False):
        if observed is None:
            return []
        return {interface["name"] for interface in observed["interfaces"]}
    return set(getattr(scope, "interface_names", []))


def _desired_acls_in_scope(desired_state, scope=None):
    if scope is not None and not getattr(scope, "full", False) and not getattr(scope, "acl_ids", []):
        return
    acl_ids = set(getattr(scope, "acl_ids", [])) if scope is not None else set()
    for acl in getattr(desired_state, "acls", []):
        if getattr(scope, "full", False) or scope is None or _field(acl, "acl_id") in acl_ids:
            yield acl


def _scoped_acl_ids(scope=None, observed=None):
    if scope is None or getattr(scope, "full", False):
        if observed is None:
            return []
        return {acl["acl_id"] for acl in observed["acls"]}
    return set(getattr(scope, "acl_ids", []))


def _desired_acl_bindings_in_scope(desired_state, scope=None):
    if (
        scope is not None
        and not getattr(scope, "full", False)
        and not getattr(scope, "interface_names", [])
        and not getattr(scope, "acl_ids", [])
    ):
        return
    interface_names = (
        {_interface_alias_key(name) for name in getattr(scope, "interface_names", [])}
        if scope is not None
        else set()
    )
    acl_ids = set(getattr(scope, "acl_ids", [])) if scope is not None else set()
    for binding in getattr(desired_state, "acl_bindings", []):
        if (
            getattr(scope, "full", False)
            or scope is None
            or _interface_alias_key(_field(binding, "interface_name")) in interface_names
            or _field(binding, "acl_id") in acl_ids
        ):
            yield binding


def _field(message, name):
    if isinstance(message, dict):
        return message[name]
    return getattr(message, name)


def _optional_field(message, name):
    if isinstance(message, dict):
        value = message.get(name)
    elif hasattr(message, "HasField"):
        try:
            if not message.HasField(name):
                return None
        except ValueError:
            pass
        value = getattr(message, name)
    else:
        value = getattr(message, name, None)
    return value if value != "" else None


def _mode_to_dict(mode) -> dict:
    kind = _field(mode, "kind")
    if kind in {"access", "ACCESS"} or int(kind or 0) == 1:
        return {
            "kind": "access",
            "access_vlan": _optional_field(mode, "access_vlan"),
            "native_vlan": None,
            "allowed_vlans": _repeated_field(mode, "allowed_vlans"),
        }
    if kind in {"trunk", "TRUNK"} or int(kind or 0) == 2:
        return {
            "kind": "trunk",
            "access_vlan": None,
            "native_vlan": _optional_field(mode, "native_vlan"),
            "allowed_vlans": sorted(set(_repeated_field(mode, "allowed_vlans"))),
        }
    raise _verify_mismatch(f"unknown port mode kind during verification: {kind!r}")


def _repeated_field(message, name) -> list:
    if isinstance(message, dict):
        return list(message.get(name, []))
    return list(getattr(message, name, []))


def _normalize_mode(mode) -> dict:
    return {
        "kind": mode.get("kind"),
        "access_vlan": mode.get("access_vlan"),
        "native_vlan": mode.get("native_vlan"),
        "allowed_vlans": sorted(set(mode.get("allowed_vlans", []))),
    }


def _normalize_acl_rules(rules) -> list[dict]:
    normalized = []
    for rule in rules:
        normalized.append(
            {
                "sequence": int(_field(rule, "sequence")),
                "action": _acl_action_text(_field(rule, "action")),
                "protocol": _acl_protocol_text(_field(rule, "protocol")),
                "source": _acl_endpoint_dict(_optional_field(rule, "source")),
                "destination": _acl_endpoint_dict(_optional_field(rule, "destination")),
                "source_port_eq": _optional_field(rule, "source_port_eq"),
                "destination_port_eq": _optional_field(rule, "destination_port_eq"),
                "description": _optional_field(rule, "description"),
            }
        )
    return sorted(normalized, key=lambda item: item["sequence"])


def _acl_action_text(value) -> str:
    if isinstance(value, str):
        return value.strip().lower()
    if int(value or 0) == 1:
        return "permit"
    if int(value or 0) == 2:
        return "deny"
    raise _verify_mismatch(f"unknown ACL action during verification: {value!r}")


def _acl_protocol_text(value) -> str:
    if isinstance(value, str):
        return value.strip().lower()
    numeric = int(value or 0)
    if numeric == 1:
        return "ip"
    if numeric == 2:
        return "tcp"
    if numeric == 3:
        return "udp"
    if numeric == 4:
        return "icmp"
    raise _verify_mismatch(f"unknown ACL protocol during verification: {value!r}")


def _acl_kind_text(value, acl_id: int) -> str:
    if isinstance(value, str):
        kind = value.strip().lower()
        if kind in {"ipv4_basic", "basic"}:
            kind = "basic_ipv4"
        elif kind in {"ipv4_advanced", "advanced"}:
            kind = "advanced_ipv4"
    else:
        numeric = int(value or 0)
        if numeric == 2:
            kind = "basic_ipv4"
        elif numeric in {0, 1}:
            kind = "basic_ipv4" if 2000 <= int(acl_id) <= 2999 else "advanced_ipv4"
        else:
            raise _verify_mismatch(f"unknown ACL kind during verification: {value!r}")
    if kind == "basic_ipv4" and not 2000 <= int(acl_id) <= 2999:
        raise _verify_mismatch(f"basic IPv4 ACL ID out of range: {acl_id}")
    if kind == "advanced_ipv4" and not 3000 <= int(acl_id) <= 3999:
        raise _verify_mismatch(f"advanced IPv4 ACL ID out of range: {acl_id}")
    if kind not in {"basic_ipv4", "advanced_ipv4"}:
        raise _verify_mismatch(f"unknown ACL kind during verification: {value!r}")
    return kind


def _acl_direction_text(value) -> str:
    if isinstance(value, str):
        return value.strip().lower()
    numeric = int(value or 0)
    if numeric == 1:
        return "inbound"
    if numeric == 2:
        return "outbound"
    raise _verify_mismatch(f"unknown ACL direction during verification: {value!r}")


def _acl_endpoint_dict(endpoint) -> dict | None:
    if endpoint is None:
        return None
    return {
        "address": _field(endpoint, "address"),
        "wildcard": _field(endpoint, "wildcard"),
    }


def _interface_alias_key(name) -> str:
    text = str(name).strip()
    aliases = (
        ("GigabitEthernet", "GE"),
        ("Ten-GigabitEthernet", "XGE"),
        ("FortyGigE", "FGE"),
    )
    for long_name, short_name in aliases:
        if text.startswith(long_name):
            return f"{short_name}{text[len(long_name):]}"
    return text


def _acl_binding_key(interface_name: str, direction: str) -> tuple[str, str]:
    return (_interface_alias_key(interface_name), direction)


def _verify_mismatch(message: str) -> AdapterError:
    return AdapterError(
        code="VERIFY_FAILED",
        message="NETCONF running verification failed",
        normalized_error="running verification failed",
        raw_error_summary=message,
        retryable=False,
    )


def scope_summary(scope) -> str:
    if scope is None:
        return "none"
    return (
        f"full={getattr(scope, 'full', False)}, "
        f"vlans={list(getattr(scope, 'vlan_ids', []))}, "
        f"interfaces={list(getattr(scope, 'interface_names', []))}, "
        f"acls={list(getattr(scope, 'acl_ids', []))}"
    )
