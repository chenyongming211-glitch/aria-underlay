from __future__ import annotations

from dataclasses import dataclass


@dataclass
class NetconfBackend:
    host: str
    port: int
    username: str | None = None
    password: str | None = None

    def get_capabilities(self) -> list[str]:
        # Real ncclient integration lands in Sprint 1.
        return []

