from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable

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

    def get_capabilities(self) -> BackendCapability:
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

        try:
            with manager.connect(
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
            ) as session:
                raw = [str(capability) for capability in session.server_capabilities]
        except Exception as exc:
            raise _adapter_error_from_ncclient_exception(exc) from exc

        return capability_from_raw(raw)

    def get_current_state(self) -> dict:
        raise AdapterError(
            code="NETCONF_STATE_PARSE_NOT_IMPLEMENTED",
            message="real NETCONF running state parsing is not implemented yet",
            normalized_error="state parser missing",
            raw_error_summary="Sprint 1B only enables real capability probing",
            retryable=False,
        )

    def prepare_candidate(self) -> None:
        raise AdapterError(
            code="NETCONF_PREPARE_NOT_IMPLEMENTED",
            message="real NETCONF prepare is not implemented yet",
            normalized_error="prepare operation missing",
            raw_error_summary="candidate edit/validate lands after renderer and parser",
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
        supports_persist_id=supports_confirmed_commit,
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
