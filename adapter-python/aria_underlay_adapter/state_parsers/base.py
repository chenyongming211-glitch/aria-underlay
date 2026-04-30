from __future__ import annotations

from typing import Protocol


class RunningStateParser(Protocol):
    production_ready: bool

    def parse_running(self, xml: str, scope=None) -> dict: ...
