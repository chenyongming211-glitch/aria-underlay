import pytest
from types import SimpleNamespace

from aria_underlay_adapter.backends.mock_netconf import (
    MockNetconfBackend,
    _admin_state_to_text,
    _normalize_mode,
)
from aria_underlay_adapter.errors import AdapterError


@pytest.mark.parametrize(
    ("profile", "supports_candidate", "supports_confirmed_commit", "backends"),
    [
        ("confirmed", True, True, ["netconf"]),
        ("lock_failed", True, True, ["netconf"]),
        ("validate_failed", True, True, ["netconf"]),
        ("commit_failed", True, True, ["netconf"]),
        ("verify_failed", True, True, ["netconf"]),
        ("candidate_only", True, False, ["netconf"]),
        ("running_only", False, False, ["netconf"]),
        ("cli_only", False, False, ["cli"]),
        ("unsupported", False, False, ["netconf"]),
    ],
)
def test_mock_capability_profiles(
    profile, supports_candidate, supports_confirmed_commit, backends
):
    capability = MockNetconfBackend(profile).get_capabilities()

    assert capability.supports_candidate is supports_candidate
    assert capability.supports_confirmed_commit is supports_confirmed_commit
    assert capability.supported_backends == backends


@pytest.mark.parametrize(
    ("profile", "code", "retryable"),
    [
        ("auth_failed", "AUTH_FAILED", False),
        ("unreachable", "DEVICE_UNREACHABLE", True),
    ],
)
def test_mock_error_profiles(profile, code, retryable):
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend(profile).get_capabilities()

    assert exc.value.code == code
    assert exc.value.retryable is retryable


def test_mock_current_state_contains_vlan_and_interface():
    state = MockNetconfBackend("confirmed").get_current_state()

    assert state["vlans"][0]["vlan_id"] == 100
    assert state["interfaces"][0]["name"] == "GE1/0/1"


def test_mock_current_state_filters_by_scope():
    scope = SimpleNamespace(
        full=False,
        vlan_ids=[200],
        interface_names=["GE1/0/99"],
    )

    state = MockNetconfBackend("confirmed").get_current_state(scope=scope)

    assert state["vlans"] == []
    assert state["interfaces"] == []


def test_mock_current_state_full_scope_returns_all():
    scope = SimpleNamespace(full=True, vlan_ids=[], interface_names=[])

    state = MockNetconfBackend("confirmed").get_current_state(scope=scope)

    assert state["vlans"][0]["vlan_id"] == 100
    assert state["interfaces"][0]["name"] == "GE1/0/1"


def test_mock_admin_state_text_matches_netconf_default_for_unspecified_values():
    assert _admin_state_to_text(0) == "up"
    assert _admin_state_to_text(None) == "up"
    assert _admin_state_to_text("DOWN") == "down"


def test_lock_failed_profile_fails_prepare():
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("lock_failed").prepare_candidate()

    assert exc.value.code == "LOCK_FAILED"


def test_validate_failed_profile_fails_prepare():
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("validate_failed").prepare_candidate()

    assert exc.value.code == "VALIDATE_FAILED"


def test_commit_failed_profile_fails_commit():
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("commit_failed").commit_candidate()

    assert exc.value.code == "COMMIT_FAILED"


def test_commit_candidate_publishes_candidate_state():
    backend = MockNetconfBackend("confirmed")
    desired = _desired_state(vlan_name="tenant-100")

    backend.prepare_candidate(desired)
    backend.commit_candidate(strategy=1, tx_id="tx-1")

    state = backend.get_current_state()
    assert state["vlans"][0]["name"] == "tenant-100"


def test_rollback_confirmed_commit_restores_previous_running_state():
    backend = MockNetconfBackend("confirmed")
    desired = _desired_state(vlan_name="tenant-100")

    backend.prepare_candidate(desired)
    backend.commit_candidate(strategy=1, tx_id="tx-1")
    backend.rollback_candidate(strategy=1, tx_id="tx-1")

    state = backend.get_current_state()
    assert state["vlans"][0]["name"] == "prod"


def test_final_confirm_keeps_confirmed_running_state():
    backend = MockNetconfBackend("confirmed")
    desired = _desired_state(vlan_name="tenant-100")

    backend.prepare_candidate(desired)
    backend.commit_candidate(strategy=1, tx_id="tx-1")
    backend.final_confirm(tx_id="tx-1")
    backend.rollback_candidate(strategy=1, tx_id="tx-1")

    state = backend.get_current_state()
    assert state["vlans"][0]["name"] == "tenant-100"


def test_verify_failed_profile_fails_verify():
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("verify_failed").verify_running(desired_state=None)

    assert exc.value.code == "VERIFY_FAILED"


def test_verify_running_accepts_matching_scoped_state():
    scope = SimpleNamespace(
        full=False,
        vlan_ids=[100],
        interface_names=["GE1/0/1"],
    )

    MockNetconfBackend("confirmed").verify_running(
        desired_state=_desired_state(),
        scope=scope,
    )


def test_verify_running_detects_vlan_mismatch():
    desired = _desired_state(vlan_name="wrong")
    scope = SimpleNamespace(full=False, vlan_ids=[100], interface_names=[])

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=scope,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "VLAN 100 name mismatch" in exc.value.message


def test_verify_running_detects_vlan_expected_absent():
    desired = _desired_state_without_vlan()
    scope = SimpleNamespace(full=False, vlan_ids=[100], interface_names=[])

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=scope,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "VLAN 100 should be absent" in exc.value.message


def test_verify_running_full_scope_detects_extra_vlan():
    desired = _desired_state_without_vlan()
    scope = SimpleNamespace(full=True, vlan_ids=[], interface_names=[])

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=scope,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "VLAN 100 should be absent" in exc.value.message


def test_verify_running_default_scope_detects_extra_vlan():
    desired = _desired_state_without_vlan()

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=None,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "VLAN 100 should be absent" in exc.value.message


def test_verify_running_detects_interface_mismatch():
    desired = _desired_state(interface_description="wrong")
    scope = SimpleNamespace(full=False, vlan_ids=[], interface_names=["GE1/0/1"])

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=scope,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "interface GE1/0/1 description mismatch" in exc.value.message


def test_verify_running_detects_interface_expected_absent():
    desired = _desired_state_without_interface()
    scope = SimpleNamespace(full=False, vlan_ids=[], interface_names=["GE1/0/1"])

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=scope,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "interface GE1/0/1 should be absent" in exc.value.message


def test_verify_running_full_scope_detects_extra_interface():
    desired = _desired_state_without_interface()
    scope = SimpleNamespace(full=True, vlan_ids=[], interface_names=[])

    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("confirmed").verify_running(
            desired_state=desired,
            scope=scope,
        )

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "interface GE1/0/1 should be absent" in exc.value.message


def test_verify_running_empty_scope_is_noop():
    scope = SimpleNamespace(full=False, vlan_ids=[], interface_names=[])

    MockNetconfBackend("confirmed").verify_running(
        desired_state=_desired_state(vlan_name="wrong", interface_description="wrong"),
        scope=scope,
    )


def test_mock_normalize_mode_rejects_unknown_kind():
    with pytest.raises(AdapterError) as exc:
        _normalize_mode({"kind": "hybrid", "access_vlan": 100})

    assert exc.value.code == "VERIFY_MISMATCH"
    assert "unknown port mode kind" in exc.value.message


def _desired_state(vlan_name="prod", interface_description="server uplink"):
    return SimpleNamespace(
        vlans=[
            SimpleNamespace(
                vlan_id=100,
                name=vlan_name,
                description="production vlan",
            )
        ],
        interfaces=[
            SimpleNamespace(
                name="GE1/0/1",
                admin_state=1,
                description=interface_description,
                mode=SimpleNamespace(
                    kind=1,
                    access_vlan=100,
                    native_vlan=None,
                    allowed_vlans=[],
                ),
            )
        ],
    )


def _desired_state_without_vlan():
    desired = _desired_state()
    desired.vlans = []
    return desired


def _desired_state_without_interface():
    desired = _desired_state()
    desired.interfaces = []
    return desired
