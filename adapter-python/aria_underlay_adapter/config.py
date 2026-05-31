from __future__ import annotations

import os
from dataclasses import dataclass


@dataclass(frozen=True)
class AdapterConfig:
    listen: str
    artifact_dir: str
    fake_mode: bool
    fake_profile: str
    secret_file: str | None
    tofu_known_hosts_file: str
    tls_cert_file: str | None
    tls_key_file: str | None
    tls_ca_cert_file: str | None
    gnmi_probe_enabled: bool = False
    gnmi_port: int = 57400
    gnmi_tls_enabled: bool = True
    gnmi_tls_ca_cert_file: str | None = None
    gnmi_tls_cert_file: str | None = None
    gnmi_tls_key_file: str | None = None
    yang_schema_collection_enabled: bool = False
    yang_library_dir: str | None = None

    @property
    def tls_enabled(self) -> bool:
        return self.tls_cert_file is not None and self.tls_key_file is not None

    @property
    def mtls_enabled(self) -> bool:
        return self.tls_enabled and self.tls_ca_cert_file is not None

    def validate(self) -> None:
        if (self.tls_cert_file is None) != (self.tls_key_file is None):
            raise ValueError(
                "TLS cert file and key file must both be provided or both be absent; "
                f"got cert_file={self.tls_cert_file!r}, key_file={self.tls_key_file!r}"
            )

    @classmethod
    def from_env(cls) -> "AdapterConfig":
        config = cls(
            listen=os.getenv("ARIA_UNDERLAY_ADAPTER_LISTEN", "127.0.0.1:50051"),
            artifact_dir=os.getenv(
                "ARIA_UNDERLAY_ARTIFACT_DIR",
                "/tmp/aria-underlay-adapter/artifacts",
            ),
            fake_mode=os.getenv("ARIA_UNDERLAY_ADAPTER_FAKE", "1") == "1",
            fake_profile=os.getenv("ARIA_UNDERLAY_FAKE_PROFILE", "confirmed"),
            secret_file=os.getenv("ARIA_UNDERLAY_SECRET_FILE"),
            tofu_known_hosts_file=os.getenv(
                "ARIA_UNDERLAY_TOFU_KNOWN_HOSTS_FILE",
                "/tmp/aria-underlay-adapter/tofu_known_hosts",
            ),
            tls_cert_file=os.getenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE"),
            tls_key_file=os.getenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE"),
            tls_ca_cert_file=os.getenv("ARIA_UNDERLAY_ADAPTER_TLS_CA_CERT_FILE"),
            gnmi_probe_enabled=_env_bool(
                "ARIA_UNDERLAY_GNMI_PROBE_ENABLED",
                default=False,
            ),
            gnmi_port=_env_int("ARIA_UNDERLAY_GNMI_PORT", default=57400),
            gnmi_tls_enabled=_env_bool(
                "ARIA_UNDERLAY_GNMI_TLS_ENABLED",
                default=True,
            ),
            gnmi_tls_ca_cert_file=os.getenv("ARIA_UNDERLAY_GNMI_TLS_CA_CERT_FILE"),
            gnmi_tls_cert_file=os.getenv("ARIA_UNDERLAY_GNMI_TLS_CERT_FILE"),
            gnmi_tls_key_file=os.getenv("ARIA_UNDERLAY_GNMI_TLS_KEY_FILE"),
            yang_schema_collection_enabled=_env_bool(
                "ARIA_UNDERLAY_YANG_SCHEMA_COLLECTION_ENABLED",
                default=False,
            ),
            yang_library_dir=os.getenv("ARIA_UNDERLAY_YANG_LIBRARY_DIR"),
        )
        config.validate()
        return config


def _env_bool(name: str, *, default: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _env_int(name: str, *, default: int) -> int:
    value = os.getenv(name)
    if value is None:
        return default
    return int(value)
