from __future__ import annotations

from dataclasses import dataclass

from aria_underlay_adapter.errors import AdapterError


BASE_10 = "urn:ietf:params:netconf:base:1.0"
BASE_11 = "urn:ietf:params:netconf:base:1.1"
CANDIDATE = "urn:ietf:params:netconf:capability:candidate:1.0"
VALIDATE_11 = "urn:ietf:params:netconf:capability:validate:1.1"
CONFIRMED_COMMIT_11 = "urn:ietf:params:netconf:capability:confirmed-commit:1.1"
ROLLBACK_ON_ERROR = "urn:ietf:params:netconf:capability:rollback-on-error:1.0"
WRITABLE_RUNNING = "urn:ietf:params:netconf:capability:writable-running:1.0"


@dataclass(frozen=True)
class MockCapability:
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


class MockNetconfBackend:
    def __init__(self, profile: str = "confirmed"):
        self.profile = profile

    def get_capabilities(self) -> MockCapability:
        if self.profile == "confirmed":
            return MockCapability(
                model="fake-confirmed-switch",
                os_version="fake-1.0",
                raw_capabilities=[BASE_10, BASE_11, CANDIDATE, VALIDATE_11, CONFIRMED_COMMIT_11],
                supports_netconf=True,
                supports_candidate=True,
                supports_validate=True,
                supports_confirmed_commit=True,
                supports_persist_id=True,
                supports_rollback_on_error=False,
                supports_writable_running=False,
                supported_backends=["netconf"],
            )

        if self.profile == "candidate_only":
            return MockCapability(
                model="fake-candidate-switch",
                os_version="fake-1.0",
                raw_capabilities=[BASE_10, CANDIDATE, VALIDATE_11],
                supports_netconf=True,
                supports_candidate=True,
                supports_validate=True,
                supports_confirmed_commit=False,
                supports_persist_id=False,
                supports_rollback_on_error=False,
                supports_writable_running=False,
                supported_backends=["netconf"],
            )

        if self.profile == "running_only":
            return MockCapability(
                model="fake-running-switch",
                os_version="fake-1.0",
                raw_capabilities=[BASE_10, WRITABLE_RUNNING, ROLLBACK_ON_ERROR],
                supports_netconf=True,
                supports_candidate=False,
                supports_validate=False,
                supports_confirmed_commit=False,
                supports_persist_id=False,
                supports_rollback_on_error=True,
                supports_writable_running=True,
                supported_backends=["netconf"],
            )

        if self.profile == "cli_only":
            return MockCapability(
                model="fake-cli-switch",
                os_version="fake-legacy",
                raw_capabilities=[],
                supports_netconf=False,
                supports_candidate=False,
                supports_validate=False,
                supports_confirmed_commit=False,
                supports_persist_id=False,
                supports_rollback_on_error=False,
                supports_writable_running=False,
                supported_backends=["cli"],
            )

        if self.profile == "unsupported":
            return MockCapability(
                model="fake-unsupported-switch",
                os_version="fake-legacy",
                raw_capabilities=[BASE_10],
                supports_netconf=True,
                supports_candidate=False,
                supports_validate=False,
                supports_confirmed_commit=False,
                supports_persist_id=False,
                supports_rollback_on_error=False,
                supports_writable_running=False,
                supported_backends=["netconf"],
            )

        if self.profile == "auth_failed":
            raise AdapterError(
                code="AUTH_FAILED",
                message="mock authentication failed",
                normalized_error="authentication failed",
                raw_error_summary="mock profile auth_failed",
                retryable=False,
            )

        if self.profile == "unreachable":
            raise AdapterError(
                code="DEVICE_UNREACHABLE",
                message="mock device unreachable",
                normalized_error="device unreachable",
                raw_error_summary="mock profile unreachable",
                retryable=True,
            )

        raise AdapterError(
            code="UNKNOWN_FAKE_PROFILE",
            message=f"unknown fake profile: {self.profile}",
            normalized_error="invalid adapter test profile",
            raw_error_summary=self.profile,
            retryable=False,
        )
