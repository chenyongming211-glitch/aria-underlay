from aria_underlay_adapter.secret_provider import NetconfSecret
from aria_underlay_adapter.server import _netconf_driver_from_device
from aria_underlay_adapter.drivers.netconf_backed import NetconfBackedDriver
from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2


class _SecretProvider:
    def resolve(self, secret_ref):
        return NetconfSecret(username="netconf", password="secret")


def _device(vendor_hint):
    return pb2.DeviceRef(
        device_id="leaf-a",
        management_ip="192.0.2.10",
        management_port=830,
        vendor_hint=vendor_hint,
        secret_ref="local/leaf-a",
    )


def test_real_server_driver_selection_rejects_skeleton_renderer():
    driver = _netconf_driver_from_device(_device(pb2.VENDOR_HUAWEI), _SecretProvider())

    response = driver.prepare(
        pb2.PrepareRequest(
            device=_device(pb2.VENDOR_HUAWEI),
            desired_state=pb2.DesiredDeviceState(device_id="leaf-a"),
        )
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "RENDERER_NOT_PRODUCTION_READY"


def test_real_server_driver_selection_rejects_unregistered_vendor():
    driver = _netconf_driver_from_device(_device(pb2.VENDOR_CISCO), _SecretProvider())

    response = driver.prepare(
        pb2.PrepareRequest(
            device=_device(pb2.VENDOR_CISCO),
            desired_state=pb2.DesiredDeviceState(device_id="leaf-a"),
        )
    )

    assert response.result.status == pb2.ADAPTER_OPERATION_STATUS_FAILED
    assert response.result.errors[0].code == "RENDERER_VENDOR_UNSUPPORTED"


def test_real_server_driver_selection_does_not_block_capability_probe_for_renderer():
    driver = _netconf_driver_from_device(_device(pb2.VENDOR_HUAWEI), _SecretProvider())

    assert isinstance(driver, NetconfBackedDriver)
