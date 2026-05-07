from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, Protocol

from aria_underlay_adapter.backends.base import BackendCapability
from aria_underlay_adapter.backends.base import CandidateDryRunResult
from aria_underlay_adapter.backends.netconf_errors import (
    adapter_error_from_ncclient_exception as _adapter_error_from_ncclient_exception,
)
from aria_underlay_adapter.backends.netconf_errors import (
    adapter_operation_error as _adapter_operation_error,
)
from aria_underlay_adapter.backends.netconf_hostkey import (
    KnownHostsTrustStore as _KnownHostsTrustStore,
)
from aria_underlay_adapter.backends.netconf_hostkey import (
    atomic_write_text as _atomic_write_text,
)
from aria_underlay_adapter.backends.netconf_hostkey import close_session as _close_session
from aria_underlay_adapter.backends.netconf_hostkey import (
    connect_with_known_hosts_file as _connect_with_known_hosts_file,
)
from aria_underlay_adapter.backends.netconf_hostkey import (
    connect_with_tofu as _connect_with_tofu,
)
from aria_underlay_adapter.backends.netconf_hostkey import (
    known_hosts_pattern as _known_hosts_pattern,
)
from aria_underlay_adapter.backends.netconf_hostkey import (
    remote_host_key as _remote_host_key,
)
from aria_underlay_adapter.backends.netconf_hostkey import (
    validate_known_hosts_path as _validate_known_hosts_path,
)
from aria_underlay_adapter.backends.netconf_state import build_state_filter
from aria_underlay_adapter.backends.netconf_state import (
    desired_state_is_empty as _desired_state_is_empty,
)
from aria_underlay_adapter.backends.netconf_state import (
    parse_running_state as _parse_running_state,
)
from aria_underlay_adapter.backends.netconf_state import (
    read_running_config as _read_running_config,
)
from aria_underlay_adapter.backends.netconf_state import scope_is_empty as _scope_is_empty
from aria_underlay_adapter.backends.netconf_state import scope_summary as _scope_summary
from aria_underlay_adapter.backends.netconf_state import verify_interfaces as _verify_interfaces
from aria_underlay_adapter.backends.netconf_state import verify_vlans as _verify_vlans
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.normalization import admin_state_to_text as _admin_state_to_text


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
    known_hosts_path: str | None = None
    tofu_known_hosts_path: str | None = None
    pinned_host_key_fingerprint: str | None = None
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
        if self.pinned_host_key_fingerprint:
            raise AdapterError(
                code="HOST_KEY_PINNING_UNSUPPORTED",
                message="NETCONF pinned fingerprint verification is not implemented",
                normalized_error="host key pinning unsupported",
                raw_error_summary=(
                    "DeviceRef carries a pinned fingerprint, but ncclient exposes only "
                    "known_hosts verification or exact hostkey_b64 pinning; refusing "
                    "to silently downgrade host key verification"
                ),
                retryable=False,
            )

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

        connect_args = {
            "host": self.host,
            "port": self.port,
            "username": self.username,
            "password": self.password,
            "key_filename": self.key_path,
            "hostkey_verify": self.hostkey_verify,
            "look_for_keys": self.look_for_keys,
            "allow_agent": False,
            "timeout": self.timeout_secs,
        }
        if self.passphrase:
            connect_args["passphrase"] = self.passphrase

        if self.tofu_known_hosts_path:
            return _connect_with_tofu(
                manager,
                connect_args,
                host=self.host,
                port=self.port,
                known_hosts_path=self.tofu_known_hosts_path,
                connect_strict=_connect_with_known_hosts_file,
            )

        if self.known_hosts_path:
            return _connect_with_known_hosts_file(
                manager,
                connect_args,
                self.known_hosts_path,
            )

        return manager.connect(**connect_args)

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

    def dry_run_candidate(self, desired_state=None) -> CandidateDryRunResult:
        if desired_state is None:
            raise AdapterError(
                code="MISSING_DESIRED_STATE",
                message="NETCONF dry-run requires desired state",
                normalized_error="desired state missing",
                raw_error_summary="DryRunRequest.desired_state is empty",
                retryable=False,
            )
        if _desired_state_is_empty(desired_state):
            return CandidateDryRunResult(
                changed=False,
                warnings=["desired state contains no VLAN or interface changes"],
            )

        config_xml = self._render_candidate_config(desired_state)
        return CandidateDryRunResult(
            changed=True,
            warnings=["candidate config rendered successfully; device session was not opened"],
            config_xml=config_xml,
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
                original_error = None
                candidate_changed = False

                try:
                    self._edit_candidate(session, desired_state)
                    candidate_changed = True
                    self._validate_candidate(session)
                except AdapterError as exc:
                    original_error = exc
                    self._discard_candidate_preserving_error(session, original_error)
                except Exception as exc:
                    original_error = _adapter_error_from_ncclient_exception(exc)
                    self._discard_candidate_preserving_error(session, original_error)

                unlock_error = None
                try:
                    self._unlock_candidate(session)
                except AdapterError as exc:
                    unlock_error = exc

                if original_error is not None:
                    if unlock_error is not None:
                        _append_secondary_error(original_error, "unlock", unlock_error)
                    raise original_error

                if unlock_error is not None:
                    if candidate_changed:
                        self._discard_candidate_preserving_error(session, unlock_error)
                    raise unlock_error
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

    def _discard_candidate_preserving_error(
        self,
        session,
        original_error: AdapterError,
    ) -> None:
        try:
            self._discard_candidate(session)
        except AdapterError as discard_error:
            original_raw = original_error.raw_error_summary or original_error.message
            discard_raw = discard_error.raw_error_summary or discard_error.message
            original_error.raw_error_summary = (
                f"{original_raw}; discard-changes also failed: {discard_raw}"
            )

    def _edit_candidate(self, session, desired_state) -> None:
        config_xml = self._render_candidate_config(desired_state)

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

    def _render_candidate_config(self, desired_state) -> str:
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

        return config_xml

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


def _append_secondary_error(
    original_error: AdapterError,
    operation: str,
    secondary_error: AdapterError,
) -> None:
    original_raw = original_error.raw_error_summary or original_error.message
    secondary_raw = secondary_error.raw_error_summary or secondary_error.message
    original_error.raw_error_summary = (
        f"{original_raw}; {operation} also failed: {secondary_raw}"
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
