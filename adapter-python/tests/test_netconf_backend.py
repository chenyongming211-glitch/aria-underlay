from pathlib import Path
from types import SimpleNamespace

import pytest

from aria_underlay_adapter.backends import netconf as netconf_module
from aria_underlay_adapter.backends import netconf_hostkey
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
    _adapter_error_from_ncclient_exception,
    _admin_state_to_text,
    build_state_filter,
    capability_from_raw,
)
from aria_underlay_adapter.drivers.netconf_backed import NetconfBackedDriver
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer
from aria_underlay_adapter.state_parsers.h3c import H3cStateParser


FIXTURES = Path(__file__).parent / "fixtures" / "state_parsers"


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


def test_ncclient_backend_rejects_pinned_fingerprint_without_exact_pin_support():
    backend = NcclientNetconfBackend(
        host="192.0.2.10",
        hostkey_verify=True,
        pinned_host_key_fingerprint="SHA256:abc123",
    )

    try:
        backend._connect()
    except AdapterError as error:
        assert error.code == "HOST_KEY_PINNING_UNSUPPORTED"
        assert error.retryable is False
    else:
        raise AssertionError("pinned fingerprint policy must fail closed until exact pinning exists")


def test_ncclient_backend_tofu_persists_first_seen_host_key(monkeypatch, tmp_path):
    manager = _FakeManager([_FakeSession("ssh-ed25519", "AAAAC3NzaC1lZDI1NTE5AAAAfirst")])
    monkeypatch.setitem(
        __import__("sys").modules,
        "ncclient",
        SimpleNamespace(manager=manager),
    )
    trust_store = tmp_path / "known_hosts"
    backend = NcclientNetconfBackend(
        host="192.0.2.10",
        port=830,
        hostkey_verify=True,
        tofu_known_hosts_path=str(trust_store),
    )

    session = backend._connect()

    assert session.closed is False
    assert manager.calls[0]["hostkey_verify"] is False
    assert trust_store.read_text(encoding="utf-8") == (
        "[192.0.2.10]:830 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAfirst\n"
    )


def test_ncclient_backend_tofu_uses_strict_known_hosts_after_first_use(monkeypatch, tmp_path):
    trust_store = tmp_path / "known_hosts"
    trust_store.write_text(
        "[192.0.2.10]:830 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAfirst\n",
        encoding="utf-8",
    )
    manager = _FakeManager([_FakeSession("ssh-ed25519", "unused")])
    monkeypatch.setitem(
        __import__("sys").modules,
        "ncclient",
        SimpleNamespace(manager=manager),
    )
    backend = NcclientNetconfBackend(
        host="192.0.2.10",
        port=830,
        hostkey_verify=True,
        tofu_known_hosts_path=str(trust_store),
    )

    backend._connect()

    assert manager.calls[0]["hostkey_verify"] is True
    assert manager.calls[0]["ssh_config_content"] == (
        "Host *\n"
        f"  UserKnownHostsFile {trust_store}\n"
    )


def test_ncclient_backend_omits_empty_passphrase_for_legacy_ncclient(
    monkeypatch, tmp_path
):
    manager = _FakeManager([_FakeSession("ssh-ed25519", "AAAAC3NzaC1lZDI1NTE5AAAAfirst")])
    monkeypatch.setitem(
        __import__("sys").modules,
        "ncclient",
        SimpleNamespace(manager=manager),
    )
    backend = NcclientNetconfBackend(
        host="192.0.2.10",
        port=830,
        username="netconf",
        password="secret",
        hostkey_verify=True,
        tofu_known_hosts_path=str(tmp_path / "known_hosts"),
    )

    backend._connect()

    assert "passphrase" not in manager.calls[0]


def test_ncclient_backend_tofu_fails_closed_when_trust_store_write_fails(
    monkeypatch, tmp_path
):
    session = _FakeSession("ssh-ed25519", "AAAAC3NzaC1lZDI1NTE5AAAAfirst")
    manager = _FakeManager([session])
    monkeypatch.setitem(
        __import__("sys").modules,
        "ncclient",
        SimpleNamespace(manager=manager),
    )

    def fail_write(*_args, **_kwargs):
        raise OSError("disk full")

    monkeypatch.setattr(netconf_hostkey, "atomic_write_text", fail_write)
    backend = NcclientNetconfBackend(
        host="192.0.2.10",
        port=830,
        hostkey_verify=True,
        tofu_known_hosts_path=str(tmp_path / "known_hosts"),
    )

    with pytest.raises(AdapterError) as exc_info:
        backend._connect()

    assert exc_info.value.code == "HOST_KEY_TRUST_STORE_WRITE_FAILED"
    assert session.closed is True


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


def test_netconf_driver_dry_run_requires_registered_renderer_before_device_read():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.dry_run(
        device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_UNKNOWN),
        desired_state=pb2.DesiredDeviceState(
            device_id="leaf-a",
            vlans=[pb2.VlanConfig(vlan_id=100, name="prod")],
        ),
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.changed is False
    assert response.result.errors[0].code == "RENDERER_VENDOR_UNSUPPORTED"
    assert session.calls == []


def test_netconf_driver_dry_run_rejects_skeleton_renderer_before_device_read():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.dry_run(
        device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
        desired_state=pb2.DesiredDeviceState(
            device_id="leaf-a",
            vlans=[pb2.VlanConfig(vlan_id=100, name="prod")],
        ),
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.changed is False
    assert response.result.errors[0].code == "RENDERER_NOT_PRODUCTION_READY"
    assert session.calls == []


def test_netconf_driver_dry_run_uses_configured_renderer_without_device_read():
    session = _RecordingSession()
    driver = NetconfBackedDriver(
        _BackendWithSession(session, config_renderer=_StaticRenderer("<config/>"))
    )

    response = driver.dry_run(
        device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
        desired_state=pb2.DesiredDeviceState(
            device_id="leaf-a",
            vlans=[pb2.VlanConfig(vlan_id=100, name="prod")],
        ),
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE
    assert response.result.changed is True
    assert response.result.errors == []
    assert any(
        "candidate config rendered successfully" in warning
        for warning in response.result.warnings
    )
    assert session.calls == []


def test_netconf_driver_dry_run_returns_no_change_for_empty_desired_without_renderer():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.dry_run(
        device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_UNKNOWN),
        desired_state=pb2.DesiredDeviceState(device_id="leaf-a"),
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE
    assert response.result.changed is False
    assert response.result.errors == []
    assert "desired state contains no VLAN or interface changes" in response.result.warnings
    assert session.calls == []


def test_netconf_driver_get_state_rejects_skeleton_parser_before_device_read():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(
            device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
            scope=pb2.StateScope(full=True),
        )
    )

    assert response.errors[0].code == "STATE_PARSER_NOT_PRODUCTION_READY"
    assert session.calls == []


def test_netconf_driver_get_state_requires_registered_parser_before_device_read():
    session = _RecordingSession()
    driver = NetconfBackedDriver(_BackendWithSession(session))

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(
            device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_UNKNOWN),
            scope=pb2.StateScope(full=True),
        )
    )

    assert response.errors[0].code == "STATE_PARSER_VENDOR_UNSUPPORTED"
    assert session.calls == []


def test_netconf_driver_get_state_can_use_fixture_verified_parser_when_enabled():
    session = _RecordingSession(reply=_Reply(_huawei_fixture_xml()))
    driver = NetconfBackedDriver(
        _BackendWithSession(session),
        allow_fixture_verified_parser=True,
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(
            device=pb2.DeviceRef(
                device_id="leaf-a",
                vendor_hint=pb2.VENDOR_HUAWEI,
            ),
            scope=pb2.StateScope(full=True),
        )
    )

    assert response.errors == []
    assert response.state.device_id == "leaf-a"
    assert [(vlan.vlan_id, vlan.name, vlan.description) for vlan in response.state.vlans] == [
        (100, "prod", "production vlan"),
        (200, "backup", ""),
    ]
    assert response.state.interfaces[0].name == "GE1/0/1"
    assert response.state.interfaces[0].mode.kind == pb2.PORT_MODE_KIND_ACCESS
    assert response.state.interfaces[0].mode.access_vlan == 100
    assert session.calls == [("get_config", {"source": "running"})]


def test_netconf_driver_get_state_can_use_h3c_fixture_verified_parser_when_enabled():
    session = _RecordingSession(reply=_Reply(_h3c_fixture_xml()))
    driver = NetconfBackedDriver(
        _BackendWithSession(session),
        allow_fixture_verified_parser=True,
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(
            device=pb2.DeviceRef(
                device_id="leaf-b",
                vendor_hint=pb2.VENDOR_H3C,
            ),
            scope=pb2.StateScope(full=True),
        )
    )

    assert response.errors == []
    assert response.state.device_id == "leaf-b"
    assert [vlan.vlan_id for vlan in response.state.vlans] == [100, 200]
    assert [interface.name for interface in response.state.interfaces] == [
        "GigabitEthernet1/0/1",
        "GigabitEthernet1/0/2",
    ]


def test_netconf_driver_get_state_normalizes_observed_admin_state():
    driver = NetconfBackedDriver(
        _ParsedStateBackend(
            {
                "vlans": [],
                "interfaces": [
                    _parsed_interface("GE1/0/1", None),
                    _parsed_interface("GE1/0/2", "UP"),
                    _parsed_interface("GE1/0/3", "down"),
                ],
            }
        )
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(device=pb2.DeviceRef(device_id="leaf-a"))
    )

    assert response.errors == []
    assert [interface.admin_state for interface in response.state.interfaces] == [
        pb2.ADMIN_STATE_UP,
        pb2.ADMIN_STATE_UP,
        pb2.ADMIN_STATE_DOWN,
    ]


def test_netconf_driver_get_state_rejects_unknown_observed_admin_state():
    driver = NetconfBackedDriver(
        _ParsedStateBackend(
            {
                "vlans": [],
                "interfaces": [_parsed_interface("GE1/0/1", "testing")],
            }
        )
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(device=pb2.DeviceRef(device_id="leaf-a"))
    )

    assert response.state.device_id == ""
    assert response.errors[0].code == "NETCONF_STATE_PARSE_FAILED"
    assert "interfaces[0].admin_state" in response.errors[0].raw_error_summary


def test_netconf_driver_get_state_returns_error_for_malformed_parser_output():
    driver = NetconfBackedDriver(
        _ParsedStateBackend(
            {
                "vlans": [],
                "interfaces": [
                    {
                        "name": "GE1/0/1",
                        "admin_state": "up",
                        "description": None,
                        "mode": {
                            "kind": "hybrid",
                            "access_vlan": None,
                            "allowed_vlans": [],
                        },
                    }
                ],
            }
        )
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(device=pb2.DeviceRef(device_id="leaf-a"))
    )

    assert response.state.device_id == ""
    assert response.errors[0].code == "NETCONF_STATE_PARSE_FAILED"
    assert "interfaces[0].mode" in response.errors[0].raw_error_summary


def test_netconf_driver_verify_succeeds_with_fixture_verified_parser_when_enabled():
    session = _RecordingSession(reply=_Reply(_huawei_fixture_xml()))
    driver = NetconfBackedDriver(
        _BackendWithSession(session),
        allow_fixture_verified_parser=True,
    )

    response = driver.verify(
        tx_id="tx-1",
        device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
        desired_state=_fixture_desired_state(),
        scope=pb2.StateScope(full=False, vlan_ids=[100], interface_names=["GE1/0/1"]),
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE
    assert response.result.errors == []
    assert session.calls == [
        (
            "get_config",
            {
                "source": "running",
                "filter": (
                    "subtree",
                    "<vlans><vlan><vlan-id>100</vlan-id></vlan></vlans><interfaces><interface><name>GE1/0/1</name></interface></interfaces>",
                ),
            },
        )
    ]


def test_netconf_driver_verify_reports_fixture_mismatch_when_enabled():
    session = _RecordingSession(reply=_Reply(_huawei_fixture_xml()))
    driver = NetconfBackedDriver(
        _BackendWithSession(session),
        allow_fixture_verified_parser=True,
    )
    desired = _fixture_desired_state(vlan_name="wrong")

    response = driver.verify(
        tx_id="tx-1",
        device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
        desired_state=desired,
        scope=pb2.StateScope(full=False, vlan_ids=[100]),
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.changed is False
    assert response.result.errors[0].code == "VERIFY_FAILED"
    assert "VLAN 100 name mismatch" in response.result.errors[0].raw_error_summary


def test_netconf_driver_get_state_scopes_fixture_verified_parser_when_enabled():
    session = _RecordingSession(reply=_Reply(_huawei_fixture_xml()))
    driver = NetconfBackedDriver(
        _BackendWithSession(session),
        allow_fixture_verified_parser=True,
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(
            device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
            scope=pb2.StateScope(
                full=False,
                vlan_ids=[100],
                interface_names=["GE1/0/1"],
            ),
        )
    )

    assert response.errors == []
    assert [vlan.vlan_id for vlan in response.state.vlans] == [100]
    assert [interface.name for interface in response.state.interfaces] == ["GE1/0/1"]
    assert session.calls == [
        (
            "get_config",
            {
                "source": "running",
                "filter": (
                    "subtree",
                    "<vlans><vlan><vlan-id>100</vlan-id></vlan></vlans><interfaces><interface><name>GE1/0/1</name></interface></interfaces>",
                ),
            },
        )
    ]


def test_netconf_driver_get_state_empty_scope_skips_fixture_parser_device_read():
    session = _RecordingSession(reply=_Reply(_huawei_fixture_xml()))
    driver = NetconfBackedDriver(
        _BackendWithSession(session),
        allow_fixture_verified_parser=True,
    )

    response = driver.get_current_state(
        pb2.GetCurrentStateRequest(
            device=pb2.DeviceRef(vendor_hint=pb2.VENDOR_HUAWEI),
            scope=pb2.StateScope(full=False),
        )
    )

    assert response.errors == []
    assert response.state.vlans == []
    assert response.state.interfaces == []
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


def test_dry_run_candidate_requires_desired_state_before_touching_device():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=_StaticRenderer("<config/>"))

    try:
        backend.dry_run_candidate(desired_state=None)
    except AdapterError as error:
        assert error.code == "MISSING_DESIRED_STATE"
    else:
        raise AssertionError("missing desired state should fail closed")

    assert session.calls == []


def test_dry_run_candidate_renders_config_without_touching_device():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=_StaticRenderer("<config/>"))

    result = backend.dry_run_candidate(desired_state=_desired_state())

    assert result.changed is True
    assert result.config_xml == "<config/>"
    assert any("candidate config rendered successfully" in warning for warning in result.warnings)
    assert session.calls == []


def test_dry_run_candidate_rejects_skeleton_renderer_without_touching_device():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=HuaweiRenderer())

    try:
        backend.dry_run_candidate(desired_state=_desired_state())
    except AdapterError as error:
        assert error.code == "NETCONF_RENDERER_NOT_PRODUCTION_READY"
    else:
        raise AssertionError("skeleton renderer should fail closed during dry-run")

    assert session.calls == []


def test_dry_run_candidate_maps_renderer_failure_without_touching_device():
    session = _RecordingSession()
    backend = _BackendWithSession(session, config_renderer=_FailingRenderer())

    try:
        backend.dry_run_candidate(desired_state=_desired_state())
    except AdapterError as error:
        assert error.code == "NETCONF_RENDERER_FAILED"
        assert error.retryable is False
    else:
        raise AssertionError("renderer failure should be normalized during dry-run")

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


def test_prepare_candidate_preserves_original_error_when_discard_fails():
    session = _RecordingSession(fail_discard=True)
    backend = _BackendWithSession(session)

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_RENDERER_NOT_CONFIGURED"
        assert "discard-changes also failed" in error.raw_error_summary
        assert "discard failed" in error.raw_error_summary
    else:
        raise AssertionError("original prepare error should be preserved")

    assert session.calls == [
        ("lock", "candidate"),
        ("discard_changes",),
        ("unlock", "candidate"),
    ]


def test_prepare_candidate_preserves_original_error_when_unlock_fails():
    session = _RecordingSession(fail_unlock=True)
    backend = _BackendWithSession(session)

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_RENDERER_NOT_CONFIGURED"
        assert "unlock also failed" in error.raw_error_summary
        assert "unlock failed" in error.raw_error_summary
    else:
        raise AssertionError("original prepare error should be preserved")

    assert session.calls == [
        ("lock", "candidate"),
        ("discard_changes",),
        ("unlock", "candidate"),
    ]


def test_prepare_candidate_discards_successful_candidate_when_unlock_fails():
    session = _RecordingSession(fail_unlock=True)
    backend = _BackendWithSession(session, config_renderer=_StaticRenderer("<config/>"))

    try:
        backend.prepare_candidate(desired_state=object())
    except AdapterError as error:
        assert error.code == "NETCONF_UNLOCK_FAILED"
    else:
        raise AssertionError("unlock failure after successful validate should fail closed")

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
        ("discard_changes",),
    ]


def test_netconf_driver_prepare_converts_unexpected_exception_to_failed_result():
    driver = NetconfBackedDriver(_UnexpectedPrepareBackend())

    response = driver.prepare(
        pb2.PrepareRequest(
            device=pb2.DeviceRef(device_id="leaf-a"),
            desired_state=pb2.DesiredDeviceState(device_id="leaf-a"),
        )
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.changed is False
    assert response.result.errors[0].code == "ADAPTER_INTERNAL_ERROR"
    assert "RuntimeError" in response.result.errors[0].raw_error_summary


def test_netconf_driver_discard_recovery_preserves_confirmed_commit_strategy():
    backend = _RecordingRecoveryBackend()
    driver = NetconfBackedDriver(backend)

    response = driver.recover(
        tx_id="tx-1",
        device=pb2.DeviceRef(device_id="leaf-a"),
        strategy=pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
        action=pb2.RECOVERY_ACTION_DISCARD_PREPARED_CHANGES,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_ROLLED_BACK
    assert backend.rollback_calls == [
        (pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT, "tx-1")
    ]


def test_netconf_driver_adapter_recovery_confirms_pending_confirmed_commit():
    backend = _RecordingRecoveryBackend()
    driver = NetconfBackedDriver(backend)

    response = driver.recover(
        tx_id="tx-1",
        device=pb2.DeviceRef(device_id="leaf-a"),
        strategy=pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
        action=pb2.RECOVERY_ACTION_ADAPTER_RECOVER,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_COMMITTED
    assert backend.final_confirm_calls == ["tx-1"]
    assert backend.rollback_calls == []


def test_netconf_driver_adapter_recovery_treats_consumed_persist_id_as_committed():
    backend = _RecordingRecoveryBackend(
        final_confirm_error=AdapterError(
            code="NETCONF_FINAL_CONFIRM_FAILED",
            message="NETCONF final confirm failed",
            normalized_error="final confirm failed",
            raw_error_summary="unknown persist-id tx-1",
            retryable=True,
        )
    )
    driver = NetconfBackedDriver(backend)

    response = driver.recover(
        tx_id="tx-1",
        device=pb2.DeviceRef(device_id="leaf-a"),
        strategy=pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
        action=pb2.RECOVERY_ACTION_ADAPTER_RECOVER,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_COMMITTED
    assert backend.final_confirm_calls == ["tx-1"]
    assert backend.rollback_calls == []


def test_netconf_driver_adapter_recovery_uses_structured_consumed_persist_id_code():
    backend = _RecordingRecoveryBackend(
        final_confirm_error=AdapterError(
            code="NETCONF_PERSIST_ID_ALREADY_CONSUMED",
            message="confirmed commit persist-id is no longer pending",
            retryable=False,
        )
    )
    driver = NetconfBackedDriver(backend)

    response = driver.recover(
        tx_id="tx-1",
        device=pb2.DeviceRef(device_id="leaf-a"),
        strategy=pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
        action=pb2.RECOVERY_ACTION_ADAPTER_RECOVER,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_COMMITTED
    assert backend.final_confirm_calls == ["tx-1"]
    assert backend.rollback_calls == []


def test_ncclient_authorization_error_is_not_authentication_failure():
    error = _adapter_error_from_ncclient_exception(RuntimeError("authorization denied"))

    assert error.code == "NETCONF_CONNECT_FAILED"


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
        "<vlans>"
        "<vlan><vlan-id>100</vlan-id></vlan>"
        "<vlan><vlan-id>200</vlan-id></vlan>"
        "</vlans>"
        "<interfaces>"
        "<interface><name>GE1/0/1&amp;backup</name></interface>"
        "<interface><name>GE1/0/2</name></interface>"
        "</interfaces>"
    )


def test_build_state_filter_uses_h3c_vlan_subtree_for_scoped_reads():
    scope = _Scope(
        full=False,
        vlan_ids=[6, 1003],
        interface_names=["Ten-GigabitEthernet1/0/47"],
    )

    assert build_state_filter(scope, parser=H3cStateParser(model_hint="S6800-54QF")) == (
        '<top xmlns="http://www.h3c.com/netconf/config:1.0"><VLAN/></top>'
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


def test_build_state_filter_rejects_non_integer_vlan_scope_with_context():
    scope = _Scope(full=False, vlan_ids=["not-a-vlan"], interface_names=[])

    try:
        build_state_filter(scope)
    except AdapterError as error:
        assert error.code == "INVALID_STATE_SCOPE"
        assert "scope.vlan_ids[0] must be an integer" in error.raw_error_summary
    else:
        raise AssertionError("non-integer state scope should fail closed")


def test_admin_state_text_uses_shared_default_for_unspecified_values():
    assert _admin_state_to_text(0) == "up"
    assert _admin_state_to_text(None) == "up"
    assert _admin_state_to_text("DOWN") == "down"


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
                    "<vlans><vlan><vlan-id>100</vlan-id></vlan></vlans>",
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
        assert error.code == "NETCONF_STATE_PARSE_NOT_IMPLEMENTED"
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
                    "<interfaces><interface><name>GE1/0/1</name></interface></interfaces>",
                ),
            },
        )
    ]


def test_get_current_state_uses_configured_production_parser():
    session = _RecordingSession(reply=_Reply("<data><vlan>100</vlan></data>"))
    parser = _StaticStateParser(
        state={
            "vlans": [
                {
                    "vlan_id": 100,
                    "name": "prod",
                    "description": "production vlan",
                }
            ],
            "interfaces": [],
        }
    )
    backend = _BackendWithSession(session, state_parser=parser)

    state = backend.get_current_state(scope=_Scope(False, [100], []))

    assert state["vlans"][0]["vlan_id"] == 100
    assert parser.calls[0][0] == "<data><vlan>100</vlan></data>"
    assert parser.calls[0][1].vlan_ids == [100]


def test_get_current_state_uses_h3c_vlan_subtree_filter_for_scoped_reads():
    session = _RecordingSession(
        reply=_Reply(
            """
            <data xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
              <top xmlns="http://www.h3c.com/netconf/config:1.0">
                <VLAN>
                  <AccessInterfaces>
                    <Interface><IfIndex>47</IfIndex><PVID>6</PVID></Interface>
                  </AccessInterfaces>
                  <VLANs>
                    <VLANID><ID>6</ID></VLANID>
                  </VLANs>
                </VLAN>
              </top>
            </data>
            """
        )
    )
    backend = _BackendWithSession(
        session,
        state_parser=H3cStateParser(model_hint="S6800-54QF"),
    )

    state = backend.get_current_state(
        scope=_Scope(False, [6], ["Ten-GigabitEthernet1/0/47"])
    )

    assert [vlan["vlan_id"] for vlan in state["vlans"]] == [6]
    assert [interface["name"] for interface in state["interfaces"]] == [
        "Ten-GigabitEthernet1/0/47"
    ]
    assert session.calls == [
        (
            "get_config",
            {
                "source": "running",
                "filter": (
                    "subtree",
                    '<top xmlns="http://www.h3c.com/netconf/config:1.0"><VLAN/></top>',
                ),
            },
        )
    ]


def test_get_current_state_rejects_non_production_parser():
    session = _RecordingSession()
    backend = _BackendWithSession(
        session,
        state_parser=_StaticStateParser(state={}, production_ready=False),
    )

    try:
        backend.get_current_state(scope=_Scope(False, [100], []))
    except AdapterError as error:
        assert error.code == "NETCONF_STATE_PARSER_NOT_PRODUCTION_READY"
        assert error.retryable is False
    else:
        raise AssertionError("skeleton state parser should fail closed")


def test_verify_running_succeeds_with_matching_parsed_state():
    session = _RecordingSession()
    parser = _StaticStateParser(
        state={
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
    )
    backend = _BackendWithSession(session, state_parser=parser)

    backend.verify_running(_desired_state(), scope=_Scope(False, [100], ["GE1/0/1"]))

    assert session.calls == [
        (
            "get_config",
            {
                "source": "running",
                "filter": (
                    "subtree",
                    "<vlans><vlan><vlan-id>100</vlan-id></vlan></vlans><interfaces><interface><name>GE1/0/1</name></interface></interfaces>",
                ),
            },
        )
    ]


def test_verify_running_fails_with_parsed_vlan_mismatch():
    session = _RecordingSession()
    parser = _StaticStateParser(
        state={
            "vlans": [
                {
                    "vlan_id": 100,
                    "name": "wrong",
                    "description": "production vlan",
                }
            ],
            "interfaces": [],
        }
    )
    backend = _BackendWithSession(session, state_parser=parser)

    try:
        backend.verify_running(_desired_state(), scope=_Scope(False, [100], []))
    except AdapterError as error:
        assert error.code == "VERIFY_FAILED"
        assert "name mismatch" in error.raw_error_summary
    else:
        raise AssertionError("parsed running mismatch should fail verification")


class _BackendWithSession(NcclientNetconfBackend):
    def __init__(self, session, config_renderer=None, state_parser=None):
        super().__init__(
            host="127.0.0.1",
            config_renderer=config_renderer,
            state_parser=state_parser,
        )
        object.__setattr__(self, "_session", session)

    def _connect(self):
        return self._session


class _FakeManager:
    def __init__(self, sessions):
        self.sessions = list(sessions)
        self.calls = []

    def connect(self, **kwargs):
        if "ssh_config" in kwargs:
            kwargs = dict(kwargs)
            kwargs["ssh_config_content"] = Path(kwargs["ssh_config"]).read_text(
                encoding="utf-8"
            )
        self.calls.append(kwargs)
        return self.sessions.pop(0)


class _FakeSession:
    def __init__(self, key_name, key_b64):
        self.closed = False
        key = SimpleNamespace(
            get_name=lambda: key_name,
            get_base64=lambda: key_b64,
        )
        transport = SimpleNamespace(get_remote_server_key=lambda: key)
        self._session = SimpleNamespace(_transport=transport)

    def close_session(self):
        self.closed = True


class _RecordingSession:
    def __init__(
        self,
        fail_lock=False,
        fail_commit=False,
        fail_discard=False,
        fail_unlock=False,
        reply="<data/>",
    ):
        self.calls = []
        self.fail_lock = fail_lock
        self.fail_commit = fail_commit
        self.fail_discard = fail_discard
        self.fail_unlock = fail_unlock
        self.reply = reply
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
        if self.fail_discard:
            raise RuntimeError("discard failed")

    def edit_config(self, **kwargs):
        self.calls.append(("edit_config", kwargs))

    def unlock(self, target):
        self.calls.append(("unlock", target))
        if self.fail_unlock:
            raise RuntimeError("unlock failed")

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
        return self.reply


class _StaticRenderer:
    production_ready = True

    def __init__(self, payload):
        self.payload = payload

    def render_edit_config(self, desired_state):
        return self.payload


class _FailingRenderer:
    production_ready = True

    def render_edit_config(self, desired_state):
        raise ValueError("renderer exploded")


class _StaticStateParser:
    def __init__(self, state, production_ready=True):
        self.state = state
        self.production_ready = production_ready
        self.calls = []

    def parse_running(self, xml, scope=None):
        self.calls.append((xml, scope))
        return self.state


class _ParsedStateBackend:
    def __init__(self, state):
        self.state = state

    def get_current_state(self, scope=None):
        return self.state


class _UnexpectedPrepareBackend:
    def prepare_candidate(self, desired_state=None):
        raise RuntimeError("unexpected backend failure")


class _RecordingRecoveryBackend:
    def __init__(self, final_confirm_error=None):
        self.final_confirm_error = final_confirm_error
        self.final_confirm_calls = []
        self.rollback_calls = []

    def final_confirm(self, tx_id=None):
        self.final_confirm_calls.append(tx_id)
        if self.final_confirm_error is not None:
            raise self.final_confirm_error

    def rollback_candidate(self, strategy=None, tx_id=None):
        self.rollback_calls.append((strategy, tx_id))


class _Reply:
    def __init__(self, data_xml):
        self.data_xml = data_xml


def _parsed_interface(name, admin_state):
    return {
        "name": name,
        "admin_state": admin_state,
        "description": None,
        "mode": {
            "kind": "access",
            "access_vlan": 100,
            "native_vlan": None,
            "allowed_vlans": [],
        },
    }


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


def _fixture_desired_state(vlan_name="prod"):
    return pb2.DesiredDeviceState(
        device_id="leaf-a",
        vlans=[
            pb2.VlanConfig(
                vlan_id=100,
                name=vlan_name,
                description="production vlan",
            )
        ],
        interfaces=[
            pb2.InterfaceConfig(
                name="GE1/0/1",
                admin_state=pb2.ADMIN_STATE_UP,
                description="server uplink",
                mode=pb2.PortMode(
                    kind=pb2.PORT_MODE_KIND_ACCESS,
                    access_vlan=100,
                ),
            )
        ],
    )


def _huawei_fixture_xml():
    return (FIXTURES / "huawei" / "vrp8_running.xml").read_text()


def _h3c_fixture_xml():
    return (FIXTURES / "h3c" / "comware7_running.xml").read_text()


class _Scope:
    def __init__(self, full, vlan_ids, interface_names):
        self.full = full
        self.vlan_ids = vlan_ids
        self.interface_names = interface_names
