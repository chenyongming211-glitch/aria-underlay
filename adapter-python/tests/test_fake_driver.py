from aria_underlay_adapter.drivers.fake import FakeDriver
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
