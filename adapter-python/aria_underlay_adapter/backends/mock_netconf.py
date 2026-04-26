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
        self._running = _default_running_state()
        self._candidate = None
        self._confirmed_before = None
        self._pending_confirm_tx_id = None

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

    def get_current_state(self, scope=None) -> dict:
        self.get_capabilities()
        return _filter_state_by_scope(self._running, scope)

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

    def prepare_candidate(self, desired_state=None) -> None:
        self.lock_candidate()
        try:
            self.edit_candidate()
            self._candidate = _merge_desired_state(self._running, desired_state)
            self.validate_candidate()
        except Exception:
            self._candidate = None
            self.unlock_candidate()
            raise

    def commit_candidate(
        self,
        strategy=None,
        tx_id: str | None = None,
        confirm_timeout_secs: int = 120,
    ) -> None:
        self.get_capabilities()
        if self.profile == "commit_failed":
            raise AdapterError(
                code="COMMIT_FAILED",
                message="mock candidate commit failed",
                normalized_error="candidate commit failed",
                raw_error_summary="mock profile commit_failed",
                retryable=True,
            )
        if self._candidate is not None:
            if _is_confirmed_commit_strategy(strategy):
                self._confirmed_before = _clone_state(self._running)
                self._pending_confirm_tx_id = tx_id
            self._running = _clone_state(self._candidate)
            self._candidate = None

    def final_confirm(self, tx_id: str | None = None) -> None:
        self.get_capabilities()
        if self._pending_confirm_tx_id and tx_id and self._pending_confirm_tx_id != tx_id:
            raise AdapterError(
                code="PERSIST_ID_MISMATCH",
                message="mock final confirm persist-id mismatch",
                normalized_error="persist-id mismatch",
                raw_error_summary=(
                    f"expected {self._pending_confirm_tx_id}, got {tx_id}"
                ),
                retryable=False,
            )
        self._confirmed_before = None
        self._pending_confirm_tx_id = None

    def rollback_candidate(self, strategy=None, tx_id: str | None = None) -> None:
        self.get_capabilities()
        if _is_confirmed_commit_strategy(strategy) and self._confirmed_before is not None:
            if self._pending_confirm_tx_id and tx_id and self._pending_confirm_tx_id != tx_id:
                raise AdapterError(
                    code="PERSIST_ID_MISMATCH",
                    message="mock cancel-commit persist-id mismatch",
                    normalized_error="persist-id mismatch",
                    raw_error_summary=(
                        f"expected {self._pending_confirm_tx_id}, got {tx_id}"
                    ),
                    retryable=False,
                )
            self._running = _clone_state(self._confirmed_before)
            self._confirmed_before = None
            self._pending_confirm_tx_id = None
        self._candidate = None

    def verify_running(self, desired_state, scope=None) -> None:
        self.get_capabilities()
        if self.profile == "verify_failed":
            raise AdapterError(
                code="VERIFY_FAILED",
                message="mock running verification failed",
                normalized_error="running verification failed",
                raw_error_summary="mock profile verify_failed",
                retryable=False,
            )
        if desired_state is None:
            return

        observed = self.get_current_state(scope=scope)
        _verify_vlans(desired_state, observed, scope)
        _verify_interfaces(desired_state, observed, scope)


def _filter_state_by_scope(state: dict, scope=None) -> dict:
    if scope is None or getattr(scope, "full", False):
        return state

    vlan_ids = set(getattr(scope, "vlan_ids", []))
    interface_names = set(getattr(scope, "interface_names", []))

    return {
        "vlans": [
            vlan
            for vlan in state["vlans"]
            if vlan["vlan_id"] in vlan_ids
        ],
        "interfaces": [
            interface
            for interface in state["interfaces"]
            if interface["name"] in interface_names
        ],
    }


def _default_running_state() -> dict:
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


def _clone_state(state: dict) -> dict:
    return {
        "vlans": [dict(vlan) for vlan in state["vlans"]],
        "interfaces": [
            {**interface, "mode": dict(interface["mode"])}
            for interface in state["interfaces"]
        ],
    }


def _merge_desired_state(running: dict, desired_state) -> dict:
    if desired_state is None:
        return _clone_state(running)

    merged = _clone_state(running)
    vlans_by_id = {vlan["vlan_id"]: vlan for vlan in merged["vlans"]}
    for desired_vlan in getattr(desired_state, "vlans", []):
        vlan_id = _field(desired_vlan, "vlan_id")
        vlans_by_id[vlan_id] = {
            "vlan_id": vlan_id,
            "name": _optional_field(desired_vlan, "name"),
            "description": _optional_field(desired_vlan, "description"),
        }
    merged["vlans"] = [
        vlans_by_id[vlan_id]
        for vlan_id in sorted(vlans_by_id)
    ]

    interfaces_by_name = {
        interface["name"]: interface
        for interface in merged["interfaces"]
    }
    for desired_interface in getattr(desired_state, "interfaces", []):
        name = _field(desired_interface, "name")
        interfaces_by_name[name] = {
            "name": name,
            "admin_state": _admin_state_to_text(_field(desired_interface, "admin_state")),
            "description": _optional_field(desired_interface, "description"),
            "mode": _mode_to_dict(_field(desired_interface, "mode")),
        }
    merged["interfaces"] = [
        interfaces_by_name[name]
        for name in sorted(interfaces_by_name)
    ]
    return merged


def _is_confirmed_commit_strategy(strategy) -> bool:
    if strategy in {"confirmed_commit", "CONFIRMED_COMMIT"}:
        return True
    return int(strategy or 0) == 1


def _verify_vlans(desired_state, observed: dict, scope=None) -> None:
    observed_by_id = {vlan["vlan_id"]: vlan for vlan in observed["vlans"]}
    desired_by_id = {
        _field(vlan, "vlan_id"): vlan
        for vlan in getattr(desired_state, "vlans", [])
    }
    for vlan_id in _scoped_vlan_ids(scope):
        if vlan_id not in desired_by_id and vlan_id in observed_by_id:
            raise _verify_mismatch(
                f"VLAN {vlan_id} should be absent but exists in observed scoped state",
            )
    for desired_vlan in _desired_vlans_in_scope(desired_state, scope):
        vlan_id = _field(desired_vlan, "vlan_id")
        observed_vlan = observed_by_id.get(vlan_id)
        if observed_vlan is None:
            raise _verify_mismatch(
                f"VLAN {vlan_id} missing from observed scoped state",
            )
        expected_name = _optional_field(desired_vlan, "name")
        expected_description = _optional_field(desired_vlan, "description")
        if observed_vlan.get("name") != expected_name:
            raise _verify_mismatch(
                f"VLAN {vlan_id} name mismatch: expected {expected_name!r}, "
                f"got {observed_vlan.get('name')!r}",
            )
        if observed_vlan.get("description") != expected_description:
            raise _verify_mismatch(
                f"VLAN {vlan_id} description mismatch: expected "
                f"{expected_description!r}, got {observed_vlan.get('description')!r}",
            )


def _verify_interfaces(desired_state, observed: dict, scope=None) -> None:
    observed_by_name = {interface["name"]: interface for interface in observed["interfaces"]}
    desired_by_name = {
        _field(interface, "name"): interface
        for interface in getattr(desired_state, "interfaces", [])
    }
    for name in _scoped_interface_names(scope):
        if name not in desired_by_name and name in observed_by_name:
            raise _verify_mismatch(
                f"interface {name} should be absent but exists in observed scoped state",
            )
    for desired_interface in _desired_interfaces_in_scope(desired_state, scope):
        name = _field(desired_interface, "name")
        observed_interface = observed_by_name.get(name)
        if observed_interface is None:
            raise _verify_mismatch(
                f"interface {name} missing from observed scoped state",
            )
        expected_admin_state = _admin_state_to_text(_field(desired_interface, "admin_state"))
        expected_description = _optional_field(desired_interface, "description")
        expected_mode = _mode_to_dict(_field(desired_interface, "mode"))

        if observed_interface.get("admin_state") != expected_admin_state:
            raise _verify_mismatch(
                f"interface {name} admin state mismatch: expected "
                f"{expected_admin_state!r}, got {observed_interface.get('admin_state')!r}",
            )
        if observed_interface.get("description") != expected_description:
            raise _verify_mismatch(
                f"interface {name} description mismatch: expected "
                f"{expected_description!r}, got {observed_interface.get('description')!r}",
            )
        if _normalize_mode(observed_interface.get("mode")) != _normalize_mode(expected_mode):
            raise _verify_mismatch(
                f"interface {name} mode mismatch: expected {expected_mode!r}, "
                f"got {observed_interface.get('mode')!r}",
            )


def _desired_vlans_in_scope(desired_state, scope=None):
    if scope is not None and not getattr(scope, "full", False) and not getattr(scope, "vlan_ids", []):
        return
    vlan_ids = set(getattr(scope, "vlan_ids", [])) if scope is not None else set()
    for vlan in getattr(desired_state, "vlans", []):
        if getattr(scope, "full", False) or scope is None or _field(vlan, "vlan_id") in vlan_ids:
            yield vlan


def _scoped_vlan_ids(scope=None):
    if scope is None or getattr(scope, "full", False):
        return []
    return set(getattr(scope, "vlan_ids", []))


def _desired_interfaces_in_scope(desired_state, scope=None):
    if (
        scope is not None
        and not getattr(scope, "full", False)
        and not getattr(scope, "interface_names", [])
    ):
        return
    interface_names = (
        set(getattr(scope, "interface_names", [])) if scope is not None else set()
    )
    for interface in getattr(desired_state, "interfaces", []):
        if (
            getattr(scope, "full", False)
            or scope is None
            or _field(interface, "name") in interface_names
        ):
            yield interface


def _scoped_interface_names(scope=None):
    if scope is None or getattr(scope, "full", False):
        return []
    return set(getattr(scope, "interface_names", []))


def _field(message, name):
    if isinstance(message, dict):
        return message[name]
    return getattr(message, name)


def _optional_field(message, name):
    if isinstance(message, dict):
        value = message.get(name)
    elif hasattr(message, "HasField"):
        try:
            value = getattr(message, name) if message.HasField(name) else None
        except ValueError:
            value = getattr(message, name)
    else:
        value = getattr(message, name, None)

    return None if value == "" else value


def _admin_state_to_text(value) -> str:
    if value in {"up", "UP", 1}:
        return "up"
    if value in {"down", "DOWN", 2}:
        return "down"
    return str(value).lower()


def _mode_to_dict(mode) -> dict:
    if isinstance(mode, dict):
        return mode

    kind = _field(mode, "kind")
    if kind in {"access", "ACCESS", 1}:
        return {
            "kind": "access",
            "access_vlan": _optional_field(mode, "access_vlan"),
            "native_vlan": None,
            "allowed_vlans": [],
        }
    if kind in {"trunk", "TRUNK", 2}:
        return {
            "kind": "trunk",
            "access_vlan": None,
            "native_vlan": _optional_field(mode, "native_vlan"),
            "allowed_vlans": list(getattr(mode, "allowed_vlans", [])),
        }
    return {"kind": kind}


def _normalize_mode(mode) -> dict:
    if mode["kind"] == "trunk":
        return {
            "kind": "trunk",
            "access_vlan": None,
            "native_vlan": mode.get("native_vlan"),
            "allowed_vlans": sorted(set(mode.get("allowed_vlans", []))),
        }
    return {
        "kind": "access",
        "access_vlan": mode.get("access_vlan"),
        "native_vlan": None,
        "allowed_vlans": [],
    }


def _verify_mismatch(message: str) -> AdapterError:
    return AdapterError(
        code="VERIFY_MISMATCH",
        message=message,
        normalized_error="running state does not match desired state",
        raw_error_summary=message,
        retryable=False,
    )
