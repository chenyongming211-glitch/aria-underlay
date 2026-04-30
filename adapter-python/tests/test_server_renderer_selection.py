from aria_underlay_adapter.secret_provider import NetconfSecret
from aria_underlay_adapter.server import _netconf_driver_from_device
from aria_underlay_adapter.drivers.netconf_backed import NetconfBackedDriver
from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2


class _SecretProvider:
    def resolve(self, secret_ref):
        return NetconfSecret(username="netconf", password="secret")


def _device(
    vendor_hint,
    *,
    host_key_policy=pb2.HOST_KEY_POLICY_TRUST_ON_FIRST_USE,
    known_hosts_path="",
    pinned_host_key_fingerprint="",
):
    return pb2.DeviceRef(
        device_id="leaf-a",
        management_ip="192.0.2.10",
        management_port=830,
        vendor_hint=vendor_hint,
        secret_ref="local/leaf-a",
        host_key_policy=host_key_policy,
        known_hosts_path=known_hosts_path,
        pinned_host_key_fingerprint=pinned_host_key_fingerprint,
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


def test_real_server_driver_selection_applies_known_hosts_policy():
    driver = _netconf_driver_from_device(
        _device(
            pb2.VENDOR_HUAWEI,
            host_key_policy=pb2.HOST_KEY_POLICY_KNOWN_HOSTS_FILE,
            known_hosts_path="/etc/aria/known_hosts",
        ),
        _SecretProvider(),
    )

    assert isinstance(driver, NetconfBackedDriver)
    assert driver._backend.hostkey_verify is True
    assert driver._backend.known_hosts_path == "/etc/aria/known_hosts"
    assert driver._backend.pinned_host_key_fingerprint is None


def test_real_server_driver_selection_applies_pinned_key_policy():
    driver = _netconf_driver_from_device(
        _device(
            pb2.VENDOR_HUAWEI,
            host_key_policy=pb2.HOST_KEY_POLICY_PINNED_KEY,
            pinned_host_key_fingerprint="SHA256:abc123",
        ),
        _SecretProvider(),
    )

    assert isinstance(driver, NetconfBackedDriver)
    assert driver._backend.hostkey_verify is True
    assert driver._backend.known_hosts_path is None
    assert driver._backend.pinned_host_key_fingerprint == "SHA256:abc123"
