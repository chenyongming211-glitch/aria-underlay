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
from aria_underlay_adapter.errors import AdapterError


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


def test_prepare_candidate_locks_discards_and_unlocks_when_edit_not_implemented():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_EDIT_CONFIG_NOT_IMPLEMENTED"
    else:
        raise AssertionError("prepare should fail closed until edit-config is implemented")

    assert session.calls == [
        ("lock", "candidate"),
        ("discard_changes",),
        ("unlock", "candidate"),
    ]


def test_prepare_candidate_lock_failure_does_not_discard_or_unlock():
    session = _RecordingSession(fail_lock=True)
    backend = _BackendWithSession(session)

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_LOCK_FAILED"
        assert error.retryable is True
    else:
        raise AssertionError("lock failure should fail closed")

    assert session.calls == [("lock", "candidate")]


def test_prepare_candidate_requires_desired_state_before_touching_device():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.prepare_candidate(desired_state=None)
    except AdapterError as error:
        assert error.code == "MISSING_DESIRED_STATE"
    else:
        raise AssertionError("missing desired state should fail closed")

    assert session.calls == []


class _BackendWithSession(NcclientNetconfBackend):
    def __init__(self, session):
        super().__init__(host="127.0.0.1")
        object.__setattr__(self, "_session", session)

    def _connect(self):
        return self._session


class _RecordingSession:
    def __init__(self, fail_lock=False):
        self.calls = []
        self.fail_lock = fail_lock
        self.server_capabilities = [BASE_10, CANDIDATE]

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def lock(self, target):
        self.calls.append(("lock", target))
        if self.fail_lock:
            raise RuntimeError("candidate already locked")

    def discard_changes(self):
        self.calls.append(("discard_changes",))

    def unlock(self, target):
        self.calls.append(("unlock", target))

    def validate(self, source):
        self.calls.append(("validate", source))
