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
        )
        config.validate()
        return config
