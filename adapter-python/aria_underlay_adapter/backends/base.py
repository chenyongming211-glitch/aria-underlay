from __future__ import annotations

from dataclasses import dataclass, field
from typing import Protocol


@dataclass(frozen=True)
class BackendCapability:
    model: str
    os_version: str
    raw_capabilities: list[str]
    supports_netconf: bool
    supports_candidate: bool
    supports_validate: bool
    supports_confirmed_commit: bool
    supports_persist_id: bool
    supports_rollback_on_error: bool
    supports_writable_running: bool
    supported_backends: list[str]
    model_profile: dict | None = None
    warnings: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class CandidateDryRunResult:
    changed: bool
    warnings: list[str]
    config_xml: str = ""


@dataclass(frozen=True)
class PreparedCandidateResult:
    candidate_checksum: str = ""


@dataclass(frozen=True)
class CandidateCommitResult:
    warnings: list[str] = field(default_factory=list)


class NetconfBackend(Protocol):
    def get_capabilities(self) -> BackendCapability: ...

    def get_current_state(self, scope=None) -> dict: ...

    def dry_run_candidate(self, desired_state=None) -> CandidateDryRunResult: ...

    def prepare_candidate(self, desired_state=None) -> PreparedCandidateResult: ...

    def commit_candidate(
        self,
        strategy=None,
        tx_id: str | None = None,
        confirm_timeout_secs: int = 120,
        prepared_candidate_checksum: str | None = None,
    ) -> CandidateCommitResult: ...

    def final_confirm(self, tx_id: str | None = None) -> None: ...

    def rollback_candidate(self, strategy=None, tx_id: str | None = None) -> None: ...

    def verify_running(self, desired_state, scope=None) -> None: ...

    def force_unlock(self, lock_owner: str, reason: str | None = None) -> None: ...
