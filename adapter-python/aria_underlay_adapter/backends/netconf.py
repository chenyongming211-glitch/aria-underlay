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
                _read_running_config(session, scope)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        raise AdapterError(
            code="NETCONF_STATE_PARSE_NOT_IMPLEMENTED",
            message="real NETCONF running state parsing is not implemented yet",
            normalized_error="state parser missing",
            raw_error_summary=(
                "scoped get-config completed, but running state parser is not implemented yet; "
                f"scope={_scope_summary(scope)}"
            ),
            retryable=False,
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
                _read_running_config(session, scope)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        raise AdapterError(
            code="NETCONF_VERIFY_NOT_IMPLEMENTED",
            message="real NETCONF running verification is not implemented yet",
            normalized_error="verification operation missing",
            raw_error_summary=(
                "scoped running get-config completed, but verification lands after state parser; "
                f"scope={_scope_summary(scope)}"
            ),
            retryable=False,
        )


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
        return session.get_config(**kwargs)
    except Exception as exc:
        raise _adapter_operation_error(
            code="NETCONF_GET_CONFIG_FAILED",
            message="NETCONF get-config running failed",
            exc=exc,
            retryable=True,
        ) from exc


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


def _scope_summary(scope) -> str:
    if scope is None:
        return "none"
    return (
        f"full={getattr(scope, 'full', False)}, "
        f"vlans={list(getattr(scope, 'vlan_ids', []))}, "
        f"interfaces={list(getattr(scope, 'interface_names', []))}"
    )
