from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable

from aria_underlay_adapter.errors import AdapterError


@dataclass(frozen=True)
class GnmiCapabilityResult:
    supported_models: list[dict[str, str]]
    encodings: list[str]
    raw_capabilities: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class GnmiCapabilityProbe:
    host: str
    port: int = 57400
    username: str | None = None
    password: str | None = None
    tls_enabled: bool = True
    tls_ca_cert_file: str | None = None
    tls_cert_file: str | None = None
    tls_key_file: str | None = None
    timeout_secs: int = 10
    client_factory: Callable[[], Any] | None = None

    def get_capabilities(self) -> GnmiCapabilityResult:
        client = self._client()
        if hasattr(client, "__enter__"):
            with client as active_client:
                return parse_gnmi_capabilities(active_client.capabilities())

        try:
            return parse_gnmi_capabilities(client.capabilities())
        finally:
            close = getattr(client, "close", None)
            if callable(close):
                close()

    def _client(self):
        if self.client_factory is not None:
            return self.client_factory()

        try:
            from pygnmi.client import gNMIclient
        except ImportError as exc:  # pragma: no cover - optional dependency
            raise RuntimeError(
                "pygnmi is not installed; install aria-underlay-adapter[gnmi] "
                "or disable ARIA_UNDERLAY_GNMI_PROBE_ENABLED"
            ) from exc

        kwargs = {
            "target": (self.host, self.port),
            "username": self.username,
            "password": self.password,
            "insecure": not self.tls_enabled,
            "gnmi_timeout": self.timeout_secs,
        }
        if self.tls_ca_cert_file:
            kwargs["path_root"] = self.tls_ca_cert_file
        if self.tls_cert_file:
            kwargs["path_cert"] = self.tls_cert_file
        if self.tls_key_file:
            kwargs["path_key"] = self.tls_key_file
        return gNMIclient(**kwargs)


def parse_gnmi_capabilities(raw_capabilities) -> GnmiCapabilityResult:
    raw = _raw_to_dict(raw_capabilities)
    supported_models = [
        _supported_model_to_dict(model)
        for model in _field(raw_capabilities, raw, "supported_models", default=[])
    ]
    encodings = [
        _encoding_to_text(encoding)
        for encoding in _field(raw_capabilities, raw, "supported_encodings", default=[])
    ]
    return GnmiCapabilityResult(
        supported_models=supported_models,
        encodings=encodings,
        raw_capabilities=raw,
    )


def _raw_to_dict(raw_capabilities) -> dict[str, Any]:
    if isinstance(raw_capabilities, dict):
        return dict(raw_capabilities)
    if hasattr(raw_capabilities, "_asdict"):
        return dict(raw_capabilities._asdict())
    return {
        "supported_models": list(getattr(raw_capabilities, "supported_models", [])),
        "supported_encodings": list(
            getattr(raw_capabilities, "supported_encodings", [])
        ),
    }


def _supported_model_to_dict(model) -> dict[str, str]:
    raw = model if isinstance(model, dict) else {}
    return {
        "name": str(_field(model, raw, "name", default="")),
        "organization": str(_field(model, raw, "organization", default="")),
        "version": str(_field(model, raw, "version", default="")),
    }


def _field(raw_object, raw_dict: dict, name: str, *, default):
    if isinstance(raw_dict, dict) and name in raw_dict:
        return raw_dict[name]
    return getattr(raw_object, name, default)


def _encoding_to_text(encoding) -> str:
    name = getattr(encoding, "name", None)
    if name:
        return str(name).lower()
    return str(encoding).lower()


def probe_error_summary(exc: Exception) -> str:
    if isinstance(exc, AdapterError):
        return exc.raw_error_summary or exc.message
    return str(exc) or type(exc).__name__
