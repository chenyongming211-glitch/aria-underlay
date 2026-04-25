from __future__ import annotations

import os
from dataclasses import dataclass


@dataclass(frozen=True)
class AdapterConfig:
    listen: str
    artifact_dir: str
    fake_mode: bool

    @classmethod
    def from_env(cls) -> "AdapterConfig":
        return cls(
            listen=os.getenv("ARIA_UNDERLAY_ADAPTER_LISTEN", "127.0.0.1:50051"),
            artifact_dir=os.getenv(
                "ARIA_UNDERLAY_ARTIFACT_DIR",
                "/tmp/aria-underlay-adapter/artifacts",
            ),
            fake_mode=os.getenv("ARIA_UNDERLAY_ADAPTER_FAKE", "1") == "1",
        )

