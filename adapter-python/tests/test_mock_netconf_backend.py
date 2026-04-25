import pytest

from aria_underlay_adapter.backends.mock_netconf import MockNetconfBackend
from aria_underlay_adapter.errors import AdapterError


@pytest.mark.parametrize(
    ("profile", "supports_candidate", "supports_confirmed_commit", "backends"),
    [
        ("confirmed", True, True, ["netconf"]),
        ("lock_failed", True, True, ["netconf"]),
        ("validate_failed", True, True, ["netconf"]),
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


def test_lock_failed_profile_fails_prepare():
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("lock_failed").prepare_candidate()

    assert exc.value.code == "LOCK_FAILED"


def test_validate_failed_profile_fails_prepare():
    with pytest.raises(AdapterError) as exc:
        MockNetconfBackend("validate_failed").prepare_candidate()

    assert exc.value.code == "VALIDATE_FAILED"
