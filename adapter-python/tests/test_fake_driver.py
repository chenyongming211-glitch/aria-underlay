from aria_underlay_adapter.drivers.fake import FakeDriver
from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2


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
