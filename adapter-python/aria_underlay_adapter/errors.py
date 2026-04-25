from __future__ import annotations

from dataclasses import dataclass


@dataclass
class AdapterError(Exception):
    code: str
    message: str
    normalized_error: str = ""
    raw_error_summary: str = ""
    retryable: bool = False

