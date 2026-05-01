from __future__ import annotations

from aria_underlay_adapter.errors import AdapterError


def adapter_error_from_ncclient_exception(exc: Exception) -> AdapterError:
    name = exc.__class__.__name__
    message = str(exc) or name
    lowered = message.lower()

    authentication_error_classes = {
        "AuthenticationError",
        "AuthenticationException",
        "SSHAuthenticationError",
    }
    authentication_phrases = (
        "authentication failed",
        "invalid username",
        "invalid password",
        "bad username or password",
    )
    if name in authentication_error_classes or any(
        phrase in lowered for phrase in authentication_phrases
    ):
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


def adapter_operation_error(
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
