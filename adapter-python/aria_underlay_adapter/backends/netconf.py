from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, Protocol
from xml.sax.saxutils import escape

from aria_underlay_adapter.backends.base import BackendCapability
from aria_underlay_adapter.errors import AdapterError


BASE_10 = "urn:ietf:params:netconf:base:1.0"
BASE_11 = "urn:ietf:params:netconf:base:1.1"
CANDIDATE = "urn:ietf:params:netconf:capability:candidate:1.0"
VALIDATE_10 = "urn:ietf:params:netconf:capability:validate:1.0"
VALIDATE_11 = "urn:ietf:params:netconf:capability:validate:1.1"
CONFIRMED_COMMIT_10 = "urn:ietf:params:netconf:capability:confirmed-commit:1.0"
CONFIRMED_COMMIT_11 = "urn:ietf:params:netconf:capability:confirmed-commit:1.1"
ROLLBACK_ON_ERROR = "urn:ietf:params:netconf:capability:rollback-on-error:1.0"
WRITABLE_RUNNING = "urn:ietf:params:netconf:capability:writable-running:1.0"
TRANSACTION_STRATEGY_CONFIRMED_COMMIT = 1
TRANSACTION_STRATEGY_CANDIDATE_COMMIT = 2


class CandidateConfigRenderer(Protocol):
    production_ready: bool

    def render_edit_config(self, desired_state) -> str: ...


class RunningStateParser(Protocol):
    production_ready: bool
    fixture_verified: bool

    def parse_running(self, xml: str, scope=None) -> dict: ...


@dataclass(frozen=True)
class NcclientNetconfBackend:
    host: str
    port: int = 830
    username: str | None = None
    password: str | None = None
    key_path: str | None = None
    passphrase: str | None = None
    hostkey_verify: bool = False
    look_for_keys: bool = False
    timeout_secs: int = 30
    config_renderer: CandidateConfigRenderer | None = None
    state_parser: RunningStateParser | None = None
    allow_fixture_verified_state_parser: bool = False

    def get_capabilities(self) -> BackendCapability:
        try:
            with self._connect() as session:
                raw = [str(capability) for capability in session.server_capabilities]
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        return capability_from_raw(raw)

    def _connect(self):
        try:
            from ncclient import manager
        except ImportError as exc:  # pragma: no cover - dependency exists in CI package
            raise AdapterError(
                code="BACKEND_DEPENDENCY_MISSING",
                message="ncclient is not installed",
                normalized_error="missing python netconf dependency",
                raw_error_summary=str(exc),
                retryable=False,
            ) from exc

        return manager.connect(
            host=self.host,
            port=self.port,
            username=self.username,
            password=self.password,
            key_filename=self.key_path,
            hostkey_verify=self.hostkey_verify,
            look_for_keys=self.look_for_keys,
            allow_agent=False,
            passphrase=self.passphrase,
            timeout=self.timeout_secs,
        )

    def get_current_state(self, scope=None) -> dict:
        if _scope_is_empty(scope):
            return {"vlans": [], "interfaces": []}

        try:
            with self._connect() as session:
                xml = _read_running_config(session, scope)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        return _parse_running_state(
            self.state_parser,
            xml,
            scope,
            allow_fixture_verified=self.allow_fixture_verified_state_parser,
        )

    def prepare_candidate(self, desired_state=None) -> None:
        if desired_state is None:
            raise AdapterError(
                code="MISSING_DESIRED_STATE",
                message="NETCONF prepare requires desired state",
                normalized_error="desired state missing",
                raw_error_summary="PrepareRequest.desired_state is empty",
                retryable=False,
            )

        try:
            with self._connect() as session:
                self._lock_candidate(session)
                try:
                    self._edit_candidate(session, desired_state)
                    self._validate_candidate(session)
                except AdapterError:
                    self._discard_candidate(session)
                    raise
                except Exception as exc:
                    self._discard_candidate(session)
                    raise _adapter_error_from_ncclient_exception(exc) from exc
                finally:
                    self._unlock_candidate(session)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

    def _lock_candidate(self, session) -> None:
        try:
            session.lock(target="candidate")
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_LOCK_FAILED",
                message="NETCONF candidate lock failed",
                exc=exc,
                retryable=True,
            ) from exc

    def _unlock_candidate(self, session) -> None:
        try:
            session.unlock(target="candidate")
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_UNLOCK_FAILED",
                message="NETCONF candidate unlock failed",
                exc=exc,
                retryable=True,
            ) from exc

    def _discard_candidate(self, session) -> None:
        try:
            session.discard_changes()
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_DISCARD_FAILED",
                message="NETCONF discard-changes failed",
                exc=exc,
                retryable=True,
            ) from exc

    def _edit_candidate(self, session, desired_state) -> None:
        if self.config_renderer is None:
            raise AdapterError(
                code="NETCONF_RENDERER_NOT_CONFIGURED",
                message="NETCONF edit-config renderer is not configured",
                normalized_error="edit-config renderer missing",
                raw_error_summary="candidate lock is wired; production renderer is required before edit-config",
                retryable=False,
            )
        if not getattr(self.config_renderer, "production_ready", False):
            raise AdapterError(
                code="NETCONF_RENDERER_NOT_PRODUCTION_READY",
                message="NETCONF edit-config renderer is not production ready",
                normalized_error="edit-config renderer not production ready",
                raw_error_summary=(
                    "renderer exists but is still a skeleton or test renderer; "
                    "refusing real edit-config"
                ),
                retryable=False,
            )

        try:
            config_xml = self.config_renderer.render_edit_config(desired_state)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_RENDERER_FAILED",
                message="NETCONF edit-config renderer failed",
                exc=exc,
                retryable=False,
            ) from exc

        if not config_xml or not config_xml.strip():
            raise AdapterError(
                code="NETCONF_EMPTY_RENDERED_CONFIG",
                message="NETCONF edit-config renderer returned empty config",
                normalized_error="empty rendered config",
                raw_error_summary="renderer output was empty",
                retryable=False,
            )

        try:
            session.edit_config(
                target="candidate",
                config=config_xml,
                default_operation="merge",
                error_option="rollback-on-error",
            )
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_EDIT_CONFIG_FAILED",
                message="NETCONF edit-config failed",
                exc=exc,
                retryable=False,
            ) from exc

    def _validate_candidate(self, session) -> None:
        try:
            session.validate(source="candidate")
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_VALIDATE_FAILED",
                message="NETCONF candidate validate failed",
                exc=exc,
                retryable=False,
            ) from exc

    def commit_candidate(
        self,
        strategy=None,
        tx_id: str | None = None,
        confirm_timeout_secs: int = 120,
    ) -> None:
        if strategy == TRANSACTION_STRATEGY_CONFIRMED_COMMIT:
            if not tx_id:
                raise AdapterError(
                    code="MISSING_TX_ID",
                    message="NETCONF confirmed-commit requires tx_id as persist token",
                    normalized_error="tx_id missing",
                    raw_error_summary="CommitRequest.context.tx_id is empty",
                    retryable=False,
                )
            try:
                with self._connect() as session:
                    session.commit(
                        confirmed=True,
                        timeout=confirm_timeout_secs or 120,
                        persist=tx_id,
                    )
            except AdapterError:
                raise
            except Exception as exc:
                raise _adapter_operation_error(
                    code="NETCONF_CONFIRMED_COMMIT_FAILED",
                    message="NETCONF confirmed-commit failed",
                    exc=exc,
                    retryable=True,
                ) from exc
            return

        if strategy != TRANSACTION_STRATEGY_CANDIDATE_COMMIT:
            raise AdapterError(
                code="NETCONF_COMMIT_STRATEGY_UNSUPPORTED",
                message="NETCONF commit strategy is unsupported",
                normalized_error="unsupported commit strategy",
                raw_error_summary=f"strategy={strategy!r}, tx_id={tx_id or ''}",
                retryable=False,
            )

        try:
            with self._connect() as session:
                session.commit()
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_COMMIT_FAILED",
                message="NETCONF candidate commit failed",
                exc=exc,
                retryable=True,
            ) from exc

    def final_confirm(self, tx_id: str | None = None) -> None:
        if not tx_id:
            raise AdapterError(
                code="MISSING_TX_ID",
                message="NETCONF final confirm requires tx_id as persist-id",
                normalized_error="tx_id missing",
                raw_error_summary="FinalConfirmRequest.context.tx_id is empty",
                retryable=False,
            )

        try:
            with self._connect() as session:
                session.commit(persist_id=tx_id)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_FINAL_CONFIRM_FAILED",
                message="NETCONF final confirm failed",
                exc=exc,
                retryable=True,
            ) from exc

    def rollback_candidate(self, strategy=None, tx_id: str | None = None) -> None:
        if strategy == TRANSACTION_STRATEGY_CONFIRMED_COMMIT:
            if not tx_id:
                raise AdapterError(
                    code="MISSING_TX_ID",
                    message="NETCONF cancel-commit requires tx_id as persist-id",
                    normalized_error="tx_id missing",
                    raw_error_summary="RollbackRequest.context.tx_id is empty",
                    retryable=False,
                )
            try:
                with self._connect() as session:
                    session.cancel_commit(persist_id=tx_id)
            except AdapterError:
                raise
            except Exception as exc:
                raise _adapter_operation_error(
                    code="NETCONF_CANCEL_COMMIT_FAILED",
                    message="NETCONF cancel-commit failed",
                    exc=exc,
                    retryable=True,
                ) from exc
            return

        if strategy == TRANSACTION_STRATEGY_CANDIDATE_COMMIT:
            try:
                with self._connect() as session:
                    session.discard_changes()
            except AdapterError:
                raise
            except Exception as exc:
                raise _adapter_operation_error(
                    code="NETCONF_DISCARD_FAILED",
                    message="NETCONF discard-changes failed",
                    exc=exc,
                    retryable=True,
                ) from exc
            return

        raise AdapterError(
            code="NETCONF_ROLLBACK_STRATEGY_UNSUPPORTED",
            message="NETCONF rollback strategy is unsupported",
            normalized_error="unsupported rollback strategy",
            raw_error_summary=f"strategy={strategy!r}, tx_id={tx_id or ''}",
            retryable=False,
        )

    def verify_running(self, desired_state, scope=None) -> None:
        if _scope_is_empty(scope):
            return

        try:
            with self._connect() as session:
                xml = _read_running_config(session, scope)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        observed = _parse_running_state(
            self.state_parser,
            xml,
            scope,
            allow_fixture_verified=self.allow_fixture_verified_state_parser,
        )
        _verify_vlans(desired_state, observed, scope)
        _verify_interfaces(desired_state, observed, scope)


NetconfBackend = NcclientNetconfBackend


def capability_from_raw(raw_capabilities: Iterable[str]) -> BackendCapability:
    raw = list(raw_capabilities)
    raw_set = set(raw)
    supports_netconf = BASE_10 in raw_set or BASE_11 in raw_set
    supports_candidate = CANDIDATE in raw_set
    supports_validate = VALIDATE_10 in raw_set or VALIDATE_11 in raw_set
    supports_confirmed_commit = (
        CONFIRMED_COMMIT_10 in raw_set or CONFIRMED_COMMIT_11 in raw_set
    )
    supports_rollback_on_error = ROLLBACK_ON_ERROR in raw_set
    supports_writable_running = WRITABLE_RUNNING in raw_set

    return BackendCapability(
        model="",
        os_version="",
        raw_capabilities=raw,
        supports_netconf=supports_netconf,
        supports_candidate=supports_candidate,
        supports_validate=supports_validate,
        supports_confirmed_commit=supports_confirmed_commit,
        supports_persist_id=CONFIRMED_COMMIT_11 in raw_set,
        supports_rollback_on_error=supports_rollback_on_error,
        supports_writable_running=supports_writable_running,
        supported_backends=["netconf"] if supports_netconf else [],
    )


def _adapter_error_from_ncclient_exception(exc: Exception) -> AdapterError:
    name = exc.__class__.__name__
    message = str(exc) or name
    lowered = message.lower()

    if "auth" in lowered or "authentication" in lowered or name == "AuthenticationError":
        return AdapterError(
            code="AUTH_FAILED",
            message="NETCONF authentication failed",
            normalized_error="authentication failed",
            raw_error_summary=message,
            retryable=False,
        )

    if "timed out" in lowered or "timeout" in lowered:
        return AdapterError(
            code="DEVICE_UNREACHABLE",
            message="NETCONF connection timed out",
            normalized_error="device unreachable",
            raw_error_summary=message,
            retryable=True,
        )

    return AdapterError(
        code="NETCONF_CONNECT_FAILED",
        message="NETCONF connection failed",
        normalized_error="netconf connect failed",
        raw_error_summary=message,
        retryable=True,
    )


def _adapter_operation_error(
    code: str,
    message: str,
    exc: Exception,
    retryable: bool,
) -> AdapterError:
    raw = str(exc) or exc.__class__.__name__
    return AdapterError(
        code=code,
        message=message,
        normalized_error=message.lower(),
        raw_error_summary=raw,
        retryable=retryable,
    )


def build_state_filter(scope=None):
    if scope is None or getattr(scope, "full", False):
        return None

    vlan_ids = _normalized_scope_vlan_ids(scope)
    interface_names = _normalized_scope_interface_names(scope)
    if not vlan_ids and not interface_names:
        return None

    parts = ['<filter type="subtree">']
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
    parts.append("</filter>")
    return "".join(parts)


def _read_running_config(session, scope=None):
    filter_xml = build_state_filter(scope)
    kwargs = {"source": "running"}
    if filter_xml is not None:
        kwargs["filter"] = ("subtree", filter_xml)

    try:
        return _running_xml_from_reply(session.get_config(**kwargs))
    except Exception as exc:
        raise _adapter_operation_error(
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


def _parse_running_state(
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
                f"scope={_scope_summary(scope)}"
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
        raise _adapter_operation_error(
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


def _scope_is_empty(scope) -> bool:
    return (
        scope is not None
        and not getattr(scope, "full", False)
        and not list(getattr(scope, "vlan_ids", []))
        and not list(getattr(scope, "interface_names", []))
    )


def _normalized_scope_vlan_ids(scope) -> list[int]:
    vlan_ids = sorted({int(vlan_id) for vlan_id in getattr(scope, "vlan_ids", [])})
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


def _verify_vlans(desired_state, observed: dict, scope=None) -> None:
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


def _verify_interfaces(desired_state, observed: dict, scope=None) -> None:
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


def _admin_state_to_text(value) -> str:
    if isinstance(value, str):
        return value.lower()
    if int(value or 0) == 2:
        return "down"
    return "up"


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


def _scope_summary(scope) -> str:
    if scope is None:
        return "none"
    return (
        f"full={getattr(scope, 'full', False)}, "
        f"vlans={list(getattr(scope, 'vlan_ids', []))}, "
        f"interfaces={list(getattr(scope, 'interface_names', []))}"
    )
