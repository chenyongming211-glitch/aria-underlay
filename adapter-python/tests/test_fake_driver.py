from aria_underlay_adapter.drivers.fake import FakeDriver
from aria_underlay_adapter.drivers.error import AdapterErrorDriver
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
from aria_underlay_adapter.server import UnderlayAdapterService


def test_fake_driver_confirmed_profile_returns_confirmed_commit():
    response = FakeDriver(profile="confirmed").get_capabilities(request=None)

    assert response.capability.supports_candidate is True
    assert response.capability.supports_confirmed_commit is True
    assert not response.errors


def test_fake_driver_auth_failed_profile_returns_adapter_error():
    response = FakeDriver(profile="auth_failed").get_capabilities(request=None)

    assert response.errors
    assert response.errors[0].code == "AUTH_FAILED"


class _Device:
    device_id = "leaf-a"


class _Request:
    device = _Device()


class _Registry:
    def __init__(self, driver):
        self.driver = driver

    def select(self, device):
        return self.driver


def test_fake_driver_get_current_state_returns_observed_state():
    response = FakeDriver(profile="confirmed").get_current_state(_Request())

    assert response.state.device_id == "leaf-a"
    assert response.state.vlans[0].vlan_id == 100
    assert response.state.interfaces[0].name == "GE1/0/1"


def test_fake_driver_prepare_success():
    response = FakeDriver(profile="confirmed").prepare(_Request())

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_PREPARED
    assert response.result.changed is True
    assert not response.result.errors


def test_fake_driver_prepare_lock_failure():
    response = FakeDriver(profile="lock_failed").prepare(_Request())

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "LOCK_FAILED"


def test_fake_driver_commit_success():
    response = FakeDriver(profile="confirmed").commit(tx_id="tx-1", device=_Device())

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_COMMITTED
    assert response.result.changed is True
    assert not response.result.errors


def test_fake_driver_commit_failure():
    response = FakeDriver(profile="commit_failed").commit(tx_id="tx-1", device=_Device())

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "COMMIT_FAILED"


def test_fake_driver_verify_success():
    response = FakeDriver(profile="confirmed").verify(
        tx_id="tx-1",
        device=_Device(),
        desired_state=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE
    assert response.result.changed is False
    assert not response.result.errors


def test_fake_driver_verify_failure():
    response = FakeDriver(profile="verify_failed").verify(
        tx_id="tx-1",
        device=_Device(),
        desired_state=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "VERIFY_FAILED"


def test_fake_driver_rollback_success():
    response = FakeDriver(profile="confirmed").rollback(tx_id="tx-1", device=_Device())

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_ROLLED_BACK
    assert response.result.changed is True
    assert not response.result.errors


def test_invalid_port_mode_kind_returns_adapter_error():
    driver = FakeDriver(profile="confirmed")

    try:
        driver._port_mode_to_proto({"kind": "routed"})
    except AdapterError as error:
        assert error.code == "INVALID_PORT_MODE"
    else:
        raise AssertionError("invalid port mode should fail")


def test_force_unlock_calls_driver_when_break_glass_enabled():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="confirmed")))
    response = service.ForceUnlock(
        pb2.ForceUnlockRequest(
            device=pb2.DeviceRef(device_id="leaf-a"),
            lock_owner="session-1",
            reason="test",
            break_glass_enabled=True,
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "NOT_IMPLEMENTED"


def test_force_unlock_preserves_driver_failure_response():
    service = UnderlayAdapterService(
        _Registry(
            AdapterErrorDriver(
                AdapterError(
                    code="SECRET_NOT_FOUND",
                    message="secret not found",
                    retryable=False,
                )
            )
        )
    )
    response = service.ForceUnlock(
        pb2.ForceUnlockRequest(
            device=pb2.DeviceRef(device_id="leaf-a"),
            lock_owner="session-1",
            reason="test",
            break_glass_enabled=True,
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "SECRET_NOT_FOUND"


def test_service_dry_run_calls_driver_and_returns_no_change_for_empty_desired():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="confirmed")))
    response = service.DryRun(
        pb2.DryRunRequest(
            device=pb2.DeviceRef(device_id="leaf-a"),
            desired_state=pb2.DesiredDeviceState(device_id="leaf-a"),
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE
    assert response.result.changed is False
    assert list(response.result.errors) == []


def test_service_dry_run_reports_changed_for_fake_desired_update():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="confirmed")))
    response = service.DryRun(
        pb2.DryRunRequest(
            device=pb2.DeviceRef(device_id="leaf-a"),
            desired_state=pb2.DesiredDeviceState(
                device_id="leaf-a",
                vlans=[pb2.VlanConfig(vlan_id=200, name="tenant-200")],
            ),
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE
    assert response.result.changed is True
    assert list(response.result.errors) == []


def test_service_recover_requires_explicit_recovery_action():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="confirmed")))
    response = service.Recover(
        pb2.RecoverRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "RECOVERY_ACTION_UNSUPPORTED"


def test_service_recover_confirms_pending_confirmed_commit():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="confirmed")))

    prepare = service.Prepare(
        pb2.PrepareRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
            desired_state=pb2.DesiredDeviceState(
                device_id="leaf-a",
                vlans=[pb2.VlanConfig(vlan_id=200, name="tenant-200")],
            ),
        ),
        context=None,
    )
    assert prepare.result.status == pb2.ADAPTER_OPERATION_STATUS_PREPARED

    commit = service.Commit(
        pb2.CommitRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
            strategy=pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
            confirm_timeout_secs=120,
        ),
        context=None,
    )
    assert commit.result.status == pb2.ADAPTER_OPERATION_STATUS_CONFIRMED_COMMIT_PENDING

    response = service.Recover(
        pb2.RecoverRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
            strategy=pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT,
            action=pb2.RECOVERY_ACTION_ADAPTER_RECOVER,
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_COMMITTED
    assert response.result.changed is True
    assert not response.result.errors


def test_service_recover_marks_candidate_commit_recovery_in_doubt():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="candidate_only")))
    response = service.Recover(
        pb2.RecoverRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
            strategy=pb2.TRANSACTION_STRATEGY_CANDIDATE_COMMIT,
            action=pb2.RECOVERY_ACTION_ADAPTER_RECOVER,
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_IN_DOUBT
    assert response.result.errors[0].code == "CANDIDATE_COMMIT_RECOVERY_IN_DOUBT"


def test_service_commit_calls_driver():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="commit_failed")))
    response = service.Commit(
        pb2.CommitRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "COMMIT_FAILED"


def test_service_verify_calls_driver():
    service = UnderlayAdapterService(_Registry(FakeDriver(profile="verify_failed")))
    response = service.Verify(
        pb2.VerifyRequest(
            context=pb2.RequestContext(tx_id="tx-1"),
            device=pb2.DeviceRef(device_id="leaf-a"),
        ),
        context=None,
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "VERIFY_FAILED"
