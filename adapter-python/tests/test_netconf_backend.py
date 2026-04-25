from aria_underlay_adapter.backends.netconf import (
    BASE_10,
    CANDIDATE,
    CONFIRMED_COMMIT_10,
    CONFIRMED_COMMIT_11,
    ROLLBACK_ON_ERROR,
    VALIDATE_10,
    VALIDATE_11,
    WRITABLE_RUNNING,
    NcclientNetconfBackend,
    NetconfBackend,
    capability_from_raw,
)


def test_capability_from_raw_detects_confirmed_commit_11():
    capability = capability_from_raw([BASE_10, CANDIDATE, VALIDATE_11, CONFIRMED_COMMIT_11])

    assert capability.supports_netconf is True
    assert capability.supports_candidate is True
    assert capability.supports_validate is True
    assert capability.supports_confirmed_commit is True
    assert capability.supports_persist_id is True
    assert capability.supported_backends == ["netconf"]


def test_capability_from_raw_detects_legacy_confirmed_commit_10():
    capability = capability_from_raw([BASE_10, CANDIDATE, VALIDATE_10, CONFIRMED_COMMIT_10])

    assert capability.supports_validate is True
    assert capability.supports_confirmed_commit is True
    assert capability.supports_persist_id is False


def test_capability_from_raw_detects_running_rollback_profile():
    capability = capability_from_raw([BASE_10, WRITABLE_RUNNING, ROLLBACK_ON_ERROR])

    assert capability.supports_candidate is False
    assert capability.supports_writable_running is True
    assert capability.supports_rollback_on_error is True


def test_legacy_netconf_backend_name_points_to_ncclient_backend():
    assert NetconfBackend is NcclientNetconfBackend
