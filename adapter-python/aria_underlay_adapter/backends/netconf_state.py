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
    if not vlan_ids and not interface_names:
        return None

    if _parser_vendor(parser) == "h3c":
        return f'<top xmlns="{H3C_COMWARE_CONFIG_NS}"><VLAN/></top>'

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
    return state


def scope_is_empty(scope) -> bool:
    return (
        scope is not None
        and not getattr(scope, "full", False)
        and not list(getattr(scope, "vlan_ids", []))
        and not list(getattr(scope, "interface_names", []))
    )


def desired_state_is_empty(desired_state) -> bool:
    return (
        not list(getattr(desired_state, "vlans", []))
        and not list(getattr(desired_state, "interfaces", []))
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
    observed_by_name = {interface["name"]: interface for interface in observed["interfaces"]}
    desired_by_name = {
        _field(interface, "name"): interface
        for interface in getattr(desired_state, "interfaces", [])
    }
    for name in _scoped_interface_names(scope, observed):
        if name not in desired_by_name and name in observed_by_name:
            raise _verify_mismatch(
                f"interface {name} should be absent but exists in observed scoped state",
            )
    for desired_interface in _desired_interfaces_in_scope(desired_state, scope):
        name = _field(desired_interface, "name")
        observed_interface = observed_by_name.get(name)
        if observed_interface is None:
            raise _verify_mismatch(
                f"interface {name} missing from observed scoped state",
            )
        expected_admin_state = _admin_state_to_text(_field(desired_interface, "admin_state"))
        expected_description = _optional_field(desired_interface, "description")
        expected_mode = _mode_to_dict(_field(desired_interface, "mode"))

        if observed_interface.get("admin_state") != expected_admin_state:
            raise _verify_mismatch(
                f"interface {name} admin state mismatch: expected "
                f"{expected_admin_state!r}, got {observed_interface.get('admin_state')!r}",
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
        set(getattr(scope, "interface_names", [])) if scope is not None else set()
    )
    for interface in getattr(desired_state, "interfaces", []):
        if (
            getattr(scope, "full", False)
            or scope is None
            or _field(interface, "name") in interface_names
        ):
            yield interface


def _scoped_interface_names(scope=None, observed=None):
    if scope is None or getattr(scope, "full", False):
        if observed is None:
            return []
        return {interface["name"] for interface in observed["interfaces"]}
    return set(getattr(scope, "interface_names", []))


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
        f"interfaces={list(getattr(scope, 'interface_names', []))}"
    )
