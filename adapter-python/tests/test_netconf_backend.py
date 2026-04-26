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


def test_prepare_candidate_locks_discards_and_unlocks_when_renderer_missing():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_RENDERER_NOT_CONFIGURED"
    else:
        raise AssertionError("prepare should fail closed until renderer is configured")

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


def test_prepare_candidate_edits_and_validates_when_renderer_is_configured():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=_StaticRenderer("<config/>"))

    backend.prepare_candidate(desired_state=object())

    assert session.calls == [
        ("lock", "candidate"),
        (
            "edit_config",
            {
                "target": "candidate",
                "config": "<config/>",
                "default_operation": "merge",
                "error_option": "rollback-on-error",
            },
        ),
        ("validate", "candidate"),
        ("unlock", "candidate"),
    ]


def test_prepare_candidate_discards_and_unlocks_when_renderer_returns_empty_config():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=_StaticRenderer(" "))

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_EMPTY_RENDERED_CONFIG"
    else:
        raise AssertionError("empty renderer output should fail closed")

    assert session.calls == [
        ("lock", "candidate"),
        ("discard_changes",),
        ("unlock", "candidate"),
    ]


class _BackendWithSession(NcclientNetconfBackend):
    def __init__(self, session, config_renderer=None):
        super().__init__(host="127.0.0.1", config_renderer=config_renderer)
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

    def edit_config(self, **kwargs):
        self.calls.append(("edit_config", kwargs))

    def unlock(self, target):
        self.calls.append(("unlock", target))

    def validate(self, source):
        self.calls.append(("validate", source))


class _StaticRenderer:
    def __init__(self, payload):
        self.payload = payload

    def render_edit_config(self, desired_state):
        return self.payload
