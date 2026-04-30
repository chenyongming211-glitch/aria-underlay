from aria_underlay_adapter.backends.netconf import (
    BASE_10,
    CANDIDATE,
    CONFIRMED_COMMIT_10,
    CONFIRMED_COMMIT_11,
    ROLLBACK_ON_ERROR,
    TRANSACTION_STRATEGY_CANDIDATE_COMMIT,
    TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
    VALIDATE_10,
    VALIDATE_11,
    WRITABLE_RUNNING,
    NcclientNetconfBackend,
    NetconfBackend,
    build_state_filter,
    capability_from_raw,
)
from aria_underlay_adapter.drivers.netconf_backed import NetconfBackedDriver
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer


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


def test_netconf_driver_prepare_requires_registered_renderer_before_device_lock():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.prepare(
        pb2.PrepareRequest(
            device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_UNKNOWN),
            desired_state=pb2.DesiredDeviceState(
                device_id="leaf-a",
                vlans=[pb2.VlanConfig(vlan_id=100, name="prod")],
            )
        )
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.changed is False
    assert response.result.errors[0].code == "RENDERER_VENDOR_UNSUPPORTED"
    assert session.calls == []


def test_netconf_driver_prepare_rejects_skeleton_renderer_before_device_lock():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.prepare(
        pb2.PrepareRequest(
            device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
            desired_state=pb2.DesiredDeviceState(
                device_id="leaf-a",
                vlans=[pb2.VlanConfig(vlan_id=100, name="prod")],
            ),
        )
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.changed is False
    assert response.result.errors[0].code == "RENDERER_NOT_PRODUCTION_READY"
    assert session.calls == []


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


def test_prepare_candidate_rejects_skeleton_vendor_renderer_before_edit_config():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=HuaweiRenderer())

    try:
        backend.prepare_candidate(desired_state=_desired_state())
    except AdapterError as error:
        assert error.code == "NETCONF_RENDERER_NOT_PRODUCTION_READY"
    else:
        raise AssertionError("skeleton vendor renderer must not reach real edit-config")

    assert session.calls == [
        ("lock", "candidate"),
        ("discard_changes",),
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


def test_commit_candidate_commits_for_candidate_strategy():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    backend.commit_candidate(
        strategy=TRANSACTION_STRATEGY_CANDIDATE_COMMIT,
        tx_id="tx-1",
    )

    assert session.calls == [("commit", {})]


def test_commit_candidate_starts_confirmed_commit_with_persist_token():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    backend.commit_candidate(
        strategy=TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
        tx_id="tx-1",
        confirm_timeout_secs=120,
    )

    assert session.calls == [
        (
            "commit",
            {
                "confirmed": True,
                "timeout": 120,
                "persist": "tx-1",
            },
        )
    ]


def test_commit_candidate_rejects_confirmed_commit_without_tx_id():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.commit_candidate(strategy=TRANSACTION_STRATEGY_CONFIRMED_COMMIT, tx_id="")
    except AdapterError as error:
        assert error.code == "MISSING_TX_ID"
    else:
        raise AssertionError("confirmed commit requires tx_id persist token")

    assert session.calls == []


def test_commit_candidate_requires_supported_strategy_before_touching_device():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.commit_candidate(strategy=None, tx_id="tx-1")
    except AdapterError as error:
        assert error.code == "NETCONF_COMMIT_STRATEGY_UNSUPPORTED"
    else:
        raise AssertionError("unknown commit strategy should fail closed")

    assert session.calls == []


def test_commit_candidate_maps_device_commit_failure():
    session = _RecordingSession(fail_commit=True)
    backend = _BackendWithSession(session)

    try:
        backend.commit_candidate(
            strategy=TRANSACTION_STRATEGY_CANDIDATE_COMMIT,
            tx_id="tx-1",
        )
    except AdapterError as error:
        assert error.code == "NETCONF_COMMIT_FAILED"
        assert error.retryable is True
    else:
        raise AssertionError("commit failure should fail closed")

    assert session.calls == [("commit", {})]


def test_final_confirm_commits_persist_id():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    backend.final_confirm(tx_id="tx-1")

    assert session.calls == [("commit", {"persist_id": "tx-1"})]


def test_rollback_candidate_discards_candidate_strategy():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    backend.rollback_candidate(strategy=TRANSACTION_STRATEGY_CANDIDATE_COMMIT, tx_id="tx-1")

    assert session.calls == [("discard_changes",)]


def test_rollback_candidate_cancels_confirmed_commit_strategy():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    backend.rollback_candidate(strategy=TRANSACTION_STRATEGY_CONFIRMED_COMMIT, tx_id="tx-1")

    assert session.calls == [("cancel_commit", {"persist_id": "tx-1"})]


def test_build_state_filter_returns_none_for_full_scope():
    scope = _Scope(full=True, vlan_ids=[100], interface_names=["GE1/0/1"])

    assert build_state_filter(scope) is None


def test_build_state_filter_returns_none_for_empty_scope():
    scope = _Scope(full=False, vlan_ids=[], interface_names=[])

    assert build_state_filter(scope) is None


def test_build_state_filter_deduplicates_sorts_and_escapes_scope_values():
    scope = _Scope(
        full=False,
        vlan_ids=[200, 100, 100],
        interface_names=["GE1/0/2", "GE1/0/1&backup", "GE1/0/2"],
    )

    assert build_state_filter(scope) == (
        '<filter type="subtree">'
        "<vlans>"
        "<vlan><vlan-id>100</vlan-id></vlan>"
        "<vlan><vlan-id>200</vlan-id></vlan>"
        "</vlans>"
        "<interfaces>"
        "<interface><name>GE1/0/1&amp;backup</name></interface>"
        "<interface><name>GE1/0/2</name></interface>"
        "</interfaces>"
        "</filter>"
    )


def test_build_state_filter_rejects_invalid_vlan_scope():
    scope = _Scope(full=False, vlan_ids=[0, 4095], interface_names=[])

    try:
        build_state_filter(scope)
    except AdapterError as error:
        assert error.code == "INVALID_STATE_SCOPE"
        assert error.retryable is False
    else:
        raise AssertionError("invalid state scope should fail closed")


def test_get_current_state_empty_scope_returns_empty_state_without_device_read():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    state = backend.get_current_state(scope=_Scope(full=False, vlan_ids=[], interface_names=[]))

    assert state == {"vlans": [], "interfaces": []}
    assert session.calls == []


def test_get_current_state_reads_running_with_scoped_filter_then_fails_parser_closed():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.get_current_state(scope=_Scope(full=False, vlan_ids=[100], interface_names=[]))
    except AdapterError as error:
        assert error.code == "NETCONF_STATE_PARSE_NOT_IMPLEMENTED"
        assert error.retryable is False
    else:
        raise AssertionError("real state parser should remain fail-closed")

    assert session.calls == [
        (
            "get_config",
            {
                "source": "running",
                "filter": (
                    "subtree",
                    '<filter type="subtree"><vlans><vlan><vlan-id>100</vlan-id></vlan></vlans></filter>',
                ),
            },
        )
    ]


def test_get_current_state_full_scope_reads_running_without_filter():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.get_current_state(scope=_Scope(full=True, vlan_ids=[], interface_names=[]))
    except AdapterError as error:
        assert error.code == "NETCONF_STATE_PARSE_NOT_IMPLEMENTED"
    else:
        raise AssertionError("real state parser should remain fail-closed")

    assert session.calls == [("get_config", {"source": "running"})]


def test_verify_running_empty_scope_is_noop_without_device_read():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    backend.verify_running(desired_state=object(), scope=_Scope(False, [], []))

    assert session.calls == []


def test_verify_running_reads_running_with_scope_then_fails_parser_closed():
    session = _RecordingSession()
    backend = _BackendWithSession(session)

    try:
        backend.verify_running(
            desired_state=object(),
            scope=_Scope(False, [], ["GE1/0/1"]),
        )
    except AdapterError as error:
        assert error.code == "NETCONF_VERIFY_NOT_IMPLEMENTED"
        assert error.retryable is False
    else:
        raise AssertionError("real verification should remain fail-closed")

    assert session.calls == [
        (
            "get_config",
            {
                "source": "running",
                "filter": (
                    "subtree",
                    '<filter type="subtree"><interfaces><interface><name>GE1/0/1</name></interface></interfaces></filter>',
                ),
            },
        )
    ]


class _BackendWithSession(NcclientNetconfBackend):
    def __init__(self, session, config_renderer=None):
        super().__init__(host="127.0.0.1", config_renderer=config_renderer)
        object.__setattr__(self, "_session", session)

    def _connect(self):
        return self._session


class _RecordingSession:
    def __init__(self, fail_lock=False, fail_commit=False):
        self.calls = []
        self.fail_lock = fail_lock
        self.fail_commit = fail_commit
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

    def commit(self, **kwargs):
        self.calls.append(("commit", kwargs))
        if self.fail_commit:
            raise RuntimeError("commit failed")

    def cancel_commit(self, **kwargs):
        self.calls.append(("cancel_commit", kwargs))

    def get_config(self, **kwargs):
        self.calls.append(("get_config", kwargs))
        return "<data/>"


class _StaticRenderer:
    production_ready = True

    def __init__(self, payload):
        self.payload = payload

    def render_edit_config(self, desired_state):
        return self.payload


def _desired_state():
    class _Desired:
        vlans = [
            type(
                "Vlan",
                (),
                {
                    "vlan_id": 100,
                    "name": "prod",
                    "description": "production vlan",
                },
            )()
        ]
        interfaces = [
            type(
                "Interface",
                (),
                {
                    "name": "GE1/0/1",
                    "admin_state": 1,
                    "description": "server uplink",
                    "mode": {
                        "kind": "access",
                        "access_vlan": 100,
                        "allowed_vlans": [],
                    },
                },
            )()
        ]

    return _Desired()


class _Scope:
    def __init__(self, full, vlan_ids, interface_names):
        self.full = full
        self.vlan_ids = vlan_ids
        self.interface_names = interface_names
