from __future__ import annotations

from dataclasses import dataclass


@dataclass
class AdapterError(Exception):
    code: str
    message: str
    normalized_error: str = ""
    raw_error_summary: str = ""
    retryable: bool = False

    def to_proto(self, pb2):
        return pb2.AdapterError(
            code=self.code,
            message=self.message,
            normalized_error=self.normalized_error,
            raw_error_summary=self.raw_error_summary,
            retryable=self.retryable,
        )
