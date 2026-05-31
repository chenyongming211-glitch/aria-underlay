from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import Iterable, Protocol

from aria_underlay_adapter.backends.base import BackendCapability
from aria_underlay_adapter.backends.base import CandidateCommitResult
from aria_underlay_adapter.backends.base import CandidateDryRunResult
from aria_underlay_adapter.backends.base import PreparedCandidateResult
from aria_underlay_adapter.backends.netconf_errors import (
    adapter_error_from_ncclient_exception as _adapter_error_from_ncclient_exception,
)
from aria_underlay_adapter.backends.netconf_errors import (
    adapter_operation_error as _adapter_operation_error,
)
from aria_underlay_adapter.backends.gnmi_capabilities import (
    GnmiCapabilityProbe as _GnmiCapabilityProbe,
)
from aria_underlay_adapter.backends.gnmi_capabilities import (
    probe_error_summary as _probe_error_summary,
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
from aria_underlay_adapter.backends.netconf_model_profile import (
    probe_openconfig_netconf_paths as _probe_openconfig_netconf_paths,
)
from aria_underlay_adapter.backends.yang_schema import (
    collect_yang_schemas as _collect_yang_schemas,
    save_yang_library as _save_yang_library,
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
from aria_underlay_adapter.backends.netconf_state import (
    read_candidate_config as _read_candidate_config,
)
from aria_underlay_adapter.backends.netconf_state import scope_is_empty as _scope_is_empty
from aria_underlay_adapter.backends.netconf_state import scope_summary as _scope_summary
from aria_underlay_adapter.backends.netconf_state import verify_acl_bindings as _verify_acl_bindings
from aria_underlay_adapter.backends.netconf_state import verify_acls as _verify_acls
from aria_underlay_adapter.backends.netconf_state import verify_interfaces as _verify_interfaces
from aria_underlay_adapter.backends.netconf_state import verify_vlans as _verify_vlans
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.model_profile import (
    classify_model_profile,
    extract_yang_modules_from_capabilities,
)
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
TRANSACTION_STRATEGY_RUNNING_ROLLBACK_ON_ERROR = 3


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
    gnmi_capability_probe: _GnmiCapabilityProbe | None = None
    yang_schema_collection_enabled: bool = False
    yang_library_dir: str | None = None

    def get_capabilities(self) -> BackendCapability:
        yang_modules: list[dict] = []
        try:
            with self._connect() as session:
                raw = [str(capability) for capability in session.server_capabilities]
                module_only_capability = capability_from_raw(raw)
                verified_paths = _probe_openconfig_netconf_paths(
                    session,
                    module_only_capability,
                )
                if self.yang_schema_collection_enabled:
                    yang_modules = _collect_and_save_yang_schemas(
                        session,
                        raw,
                        yang_library_dir=self.yang_library_dir,
                    )
            gnmi_supported_models: list[dict[str, str]] = []
            warnings: list[str] = []
            if self.gnmi_capability_probe is not None:
                try:
                    gnmi_result = self.gnmi_capability_probe.get_capabilities()
                    gnmi_supported_models = gnmi_result.supported_models
                except Exception as exc:
                    warnings.append(
                        "gNMI capabilities probe failed: "
                        f"{_probe_error_summary(exc)}"
                    )
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        return capability_from_raw(
            raw,
            verified_paths=verified_paths,
            gnmi_supported_models=gnmi_supported_models,
            warnings=warnings,
            yang_modules=yang_modules,
        )

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
            return {"vlans": [], "interfaces": [], "acls": [], "acl_bindings": []}

        try:
            with self._connect() as session:
                xml = _read_running_config(session, scope, self.state_parser)
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
                warnings=["desired state contains no supported config changes"],
            )

        config_xml = self._render_candidate_config(desired_state)
        return CandidateDryRunResult(
            changed=True,
            warnings=["candidate config rendered successfully; device session was not opened"],
            config_xml=config_xml,
        )

    def prepare_candidate(self, desired_state=None) -> PreparedCandidateResult:
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
                capability = capability_from_raw(
                    str(capability) for capability in session.server_capabilities
                )
                if (
                    not capability.supports_candidate
                    and capability.supports_writable_running
                    and capability.supports_rollback_on_error
                ):
                    self._edit_running_with_rollback_on_error(session, desired_state)
                    return PreparedCandidateResult()
                if not capability.supports_candidate:
                    raise AdapterError(
                        code="NETCONF_PREPARE_STRATEGY_UNSUPPORTED",
                        message="NETCONF prepare requires candidate or writable-running rollback-on-error",
                        normalized_error="prepare strategy unsupported",
                        raw_error_summary=(
                            "device lacks candidate and rollback-on-error writable-running "
                            "capabilities"
                        ),
                        retryable=False,
                    )
                self._lock_candidate(session)
                original_error = None
                candidate_changed = False
                candidate_checksum = ""

                try:
                    self._edit_candidate(session, desired_state)
                    candidate_changed = True
                    self._validate_candidate(session)
                    candidate_checksum = self._candidate_checksum(session)
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
                return PreparedCandidateResult(candidate_checksum=candidate_checksum)
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

    def _edit_running_with_rollback_on_error(self, session, desired_state) -> None:
        config_xml = self._render_candidate_config(desired_state)

        try:
            session.edit_config(
                target="running",
                config=config_xml,
                default_operation="merge",
                error_option="rollback-on-error",
            )
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_EDIT_RUNNING_FAILED",
                message="NETCONF edit-config running failed",
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

    def _candidate_checksum(self, session) -> str:
        xml = _read_candidate_config(session)
        return hashlib.sha256(xml.encode("utf-8")).hexdigest()

    def commit_candidate(
        self,
        strategy=None,
        tx_id: str | None = None,
        confirm_timeout_secs: int = 120,
        prepared_candidate_checksum: str | None = None,
    ) -> CandidateCommitResult:
        if strategy == TRANSACTION_STRATEGY_CONFIRMED_COMMIT:
            if not tx_id:
                raise AdapterError(
                    code="MISSING_TX_ID",
                    message="NETCONF confirmed-commit requires tx_id as persist token",
                    normalized_error="tx_id missing",
                    raw_error_summary="CommitRequest.context.tx_id is empty",
                    retryable=False,
                )

        if strategy == TRANSACTION_STRATEGY_RUNNING_ROLLBACK_ON_ERROR:
            return CandidateCommitResult()

        if strategy not in (
            TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
            TRANSACTION_STRATEGY_CANDIDATE_COMMIT,
        ):
            raise AdapterError(
                code="NETCONF_COMMIT_STRATEGY_UNSUPPORTED",
                message="NETCONF commit strategy is unsupported",
                normalized_error="unsupported commit strategy",
                raw_error_summary=f"strategy={strategy!r}, tx_id={tx_id or ''}",
                retryable=False,
            )

        try:
            with self._connect() as session:
                return self._commit_locked_candidate(
                    session,
                    strategy=strategy,
                    tx_id=tx_id,
                    confirm_timeout_secs=confirm_timeout_secs,
                    prepared_candidate_checksum=prepared_candidate_checksum,
                )
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

    def _commit_locked_candidate(
        self,
        session,
        *,
        strategy,
        tx_id: str | None,
        confirm_timeout_secs: int,
        prepared_candidate_checksum: str | None,
    ) -> CandidateCommitResult:
        self._lock_candidate(session)
        committed = False
        original_error = None

        try:
            if prepared_candidate_checksum:
                self._verify_prepared_candidate_checksum(
                    session,
                    prepared_candidate_checksum,
                )
            self._validate_candidate(session)
            self._commit_candidate_session(
                session,
                strategy=strategy,
                tx_id=tx_id,
                confirm_timeout_secs=confirm_timeout_secs,
            )
            committed = True
        except AdapterError as exc:
            original_error = exc
        except Exception as exc:
            original_error = _adapter_error_from_ncclient_exception(exc)

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
            if committed:
                return CandidateCommitResult(
                    warnings=[
                        "candidate commit completed, but candidate unlock failed: "
                        f"{unlock_error.raw_error_summary or unlock_error.message}"
                    ]
                )
            raise unlock_error

        return CandidateCommitResult()

    def _verify_prepared_candidate_checksum(
        self,
        session,
        prepared_candidate_checksum: str,
    ) -> None:
        current_checksum = self._candidate_checksum(session)
        if current_checksum == prepared_candidate_checksum:
            return
        raise AdapterError(
            code="NETCONF_CANDIDATE_CHANGED",
            message="NETCONF candidate changed after prepare",
            normalized_error="candidate changed after prepare",
            raw_error_summary=(
                f"prepared_candidate_checksum={prepared_candidate_checksum}, "
                f"current_candidate_checksum={current_checksum}"
            ),
            retryable=False,
        )

    def _commit_candidate_session(
        self,
        session,
        *,
        strategy,
        tx_id: str | None,
        confirm_timeout_secs: int,
    ) -> None:
        if strategy == TRANSACTION_STRATEGY_CONFIRMED_COMMIT:
            try:
                session.commit(
                    confirmed=True,
                    timeout=confirm_timeout_secs or 120,
                    persist=tx_id,
                )
            except Exception as exc:
                raise _adapter_operation_error(
                    code="NETCONF_CONFIRMED_COMMIT_FAILED",
                    message="NETCONF confirmed-commit failed",
                    exc=exc,
                    retryable=True,
                ) from exc
            return

        try:
            session.commit()
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

    def force_unlock(self, lock_owner: str, reason: str | None = None) -> None:
        session_id = _force_unlock_session_id(lock_owner)
        try:
            with self._connect() as session:
                session.kill_session(session_id)
        except AdapterError:
            raise
        except Exception as exc:
            raise _adapter_operation_error(
                code="NETCONF_FORCE_UNLOCK_FAILED",
                message="NETCONF kill-session failed",
                exc=exc,
                retryable=True,
            ) from exc

    def verify_running(self, desired_state, scope=None) -> None:
        if _scope_is_empty(scope):
            return

        try:
            with self._connect() as session:
                xml = _read_running_config(session, scope, self.state_parser)
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
        _verify_acls(desired_state, observed, scope)
        _verify_acl_bindings(desired_state, observed, scope)


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


def _force_unlock_session_id(lock_owner: str | None) -> str:
    session_id = (lock_owner or "").strip()
    if session_id.isdigit() and int(session_id) > 0:
        return session_id
    raise AdapterError(
        code="NETCONF_FORCE_UNLOCK_SESSION_ID_INVALID",
        message="NETCONF force unlock requires lock_owner to be a NETCONF session-id",
        normalized_error="invalid force unlock session id",
        raw_error_summary=f"lock_owner={lock_owner!r}",
        retryable=False,
    )


NetconfBackend = NcclientNetconfBackend


def _collect_and_save_yang_schemas(
    session,
    raw_capabilities: list[str],
    *,
    yang_library_dir: str | None = None,
) -> list[dict]:
    """Collect YANG schemas from a live NETCONF session and optionally save to disk.

    Schema collection failures are non-fatal: any exception is caught and
    reported as a warning. The function always returns the per-module
    summary list so the capability response can include it.
    """
    try:
        collection = _collect_yang_schemas(session, raw_capabilities)
    except Exception:
        return []

    if yang_library_dir:
        try:
            _save_yang_library(
                collection,
                vendor="unknown",
                model="unknown",
                os_version="unknown",
                base_dir=yang_library_dir,
            )
        except Exception:
            pass

    return collection.to_summary_dicts()


def capability_from_raw(
    raw_capabilities: Iterable[str],
    *,
    verified_paths: dict[str, dict] | None = None,
    gnmi_supported_models: list[dict[str, str]] | None = None,
    warnings: list[str] | None = None,
    yang_modules: list[dict] | None = None,
) -> BackendCapability:
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
    parsed_yang_modules = extract_yang_modules_from_capabilities(raw)

    model_profile = classify_model_profile(
        vendor="unknown",
        model="unknown",
        os_version="unknown",
        supports_candidate=supports_candidate,
        supports_validate=supports_validate,
        supported_modules=parsed_yang_modules,
        verified_paths=verified_paths or {},
        gnmi_supported_models=gnmi_supported_models or [],
    )
    if yang_modules is not None:
        model_profile["yang_module_count"] = len(yang_modules)

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
        model_profile=model_profile,
        warnings=warnings or [],
        yang_modules=yang_modules or [],
    )
