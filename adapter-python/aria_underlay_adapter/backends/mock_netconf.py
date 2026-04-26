from __future__ import annotations

from aria_underlay_adapter.backends.base import BackendCapability
from aria_underlay_adapter.errors import AdapterError


BASE_10 = "urn:ietf:params:netconf:base:1.0"
BASE_11 = "urn:ietf:params:netconf:base:1.1"
CANDIDATE = "urn:ietf:params:netconf:capability:candidate:1.0"
VALIDATE_11 = "urn:ietf:params:netconf:capability:validate:1.1"
CONFIRMED_COMMIT_11 = "urn:ietf:params:netconf:capability:confirmed-commit:1.1"
ROLLBACK_ON_ERROR = "urn:ietf:params:netconf:capability:rollback-on-error:1.0"
WRITABLE_RUNNING = "urn:ietf:params:netconf:capability:writable-running:1.0"


class MockNetconfBackend:
    def __init__(self, profile: str = "confirmed"):
        self.profile = profile

    def get_capabilities(self) -> BackendCapability:
        if self.profile in {
            "confirmed",
            "lock_failed",
            "validate_failed",
            "commit_failed",
            "verify_failed",
        }:
            return BackendCapability(
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
            return BackendCapability(
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
            return BackendCapability(
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
            return BackendCapability(
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
            return BackendCapability(
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

    def get_current_state(self) -> dict:
        self.get_capabilities()
        return {
            "vlans": [
                {
                    "vlan_id": 100,
                    "name": "prod",
                    "description": "production vlan",
                }
            ],
            "interfaces": [
                {
                    "name": "GE1/0/1",
                    "admin_state": "up",
                    "description": "server uplink",
                    "mode": {
                        "kind": "access",
                        "access_vlan": 100,
                        "native_vlan": None,
                        "allowed_vlans": [],
                    },
                }
            ],
        }

    def lock_candidate(self) -> None:
        if self.profile == "lock_failed":
            raise AdapterError(
                code="LOCK_FAILED",
                message="mock candidate lock failed",
                normalized_error="candidate lock failed",
                raw_error_summary="mock profile lock_failed",
                retryable=True,
            )

    def edit_candidate(self) -> None:
        self.get_capabilities()

    def validate_candidate(self) -> None:
        if self.profile == "validate_failed":
            raise AdapterError(
                code="VALIDATE_FAILED",
                message="mock candidate validate failed",
                normalized_error="candidate validate failed",
                raw_error_summary="mock profile validate_failed",
                retryable=False,
            )

    def unlock_candidate(self) -> None:
        return None

    def prepare_candidate(self) -> None:
        self.lock_candidate()
        try:
            self.edit_candidate()
            self.validate_candidate()
        except Exception:
            self.unlock_candidate()
            raise

    def commit_candidate(self) -> None:
        self.get_capabilities()
        if self.profile == "commit_failed":
            raise AdapterError(
                code="COMMIT_FAILED",
                message="mock candidate commit failed",
                normalized_error="candidate commit failed",
                raw_error_summary="mock profile commit_failed",
                retryable=True,
            )

    def rollback_candidate(self) -> None:
        self.get_capabilities()

    def verify_running(self, desired_state) -> None:
        self.get_capabilities()
        if self.profile == "verify_failed":
            raise AdapterError(
                code="VERIFY_FAILED",
                message="mock running verification failed",
                normalized_error="running verification failed",
                raw_error_summary="mock profile verify_failed",
                retryable=False,
            )
