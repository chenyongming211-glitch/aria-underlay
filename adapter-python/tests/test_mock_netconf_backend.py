import pytest

from aria_underlay_adapter.backends.mock_netconf import MockNetconfBackend
from aria_underlay_adapter.errors import AdapterError


@pytest.mark.parametrize(
    ("profile", "supports_candidate", "supports_confirmed_commit", "backends"),
    [
        ("confirmed", True, True, ["netconf"]),
        ("candidate_only", True, False, ["netconf"]),
        ("running_only", False, False, ["netconf"]),
        ("cli_only", False, False, ["cli"]),
        ("unsupported", False, False, []),
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

