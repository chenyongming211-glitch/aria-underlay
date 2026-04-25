from __future__ import annotations

from aria_underlay_adapter.backends.mock_netconf import MockNetconfBackend
from aria_underlay_adapter.drivers.base import DeviceDriver
from aria_underlay_adapter.errors import AdapterError

try:
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
except ImportError as exc:  # pragma: no cover
    raise RuntimeError("generated protobuf modules are missing") from exc


class FakeDriver(DeviceDriver):
    def __init__(self, profile: str = "confirmed"):
        self._backend = MockNetconfBackend(profile)

    def get_capabilities(self, request):
        try:
            capability = self._backend.get_capabilities()
        except AdapterError as error:
            return pb2.GetCapabilitiesResponse(errors=[error.to_proto(pb2)])

        return pb2.GetCapabilitiesResponse(
            capability=pb2.DeviceCapability(
                vendor=pb2.VENDOR_UNKNOWN,
                model=capability.model,
                os_version=capability.os_version,
                raw_capabilities=capability.raw_capabilities,
                supports_netconf=capability.supports_netconf,
                supports_candidate=capability.supports_candidate,
                supports_validate=capability.supports_validate,
                supports_confirmed_commit=capability.supports_confirmed_commit,
                supports_persist_id=capability.supports_persist_id,
                supports_rollback_on_error=capability.supports_rollback_on_error,
                supports_writable_running=capability.supports_writable_running,
                supported_backends=[
                    self._backend_kind_to_proto(backend)
                    for backend in capability.supported_backends
                ],
            )
        )

    def _backend_kind_to_proto(self, backend: str):
        if backend == "netconf":
            return pb2.BACKEND_KIND_NETCONF
        if backend == "napalm":
            return pb2.BACKEND_KIND_NAPALM
        if backend == "netmiko":
            return pb2.BACKEND_KIND_NETMIKO
        if backend == "cli":
            return pb2.BACKEND_KIND_CLI
        return pb2.BACKEND_KIND_UNSPECIFIED

    def get_current_state(self, device, scope):
        raise NotImplementedError

    def dry_run(self, device, desired_state):
        raise NotImplementedError

    def prepare(self, tx_id, device, desired_state):
        raise NotImplementedError

    def commit(self, tx_id, device):
        raise NotImplementedError

    def rollback(self, tx_id, device):
        raise NotImplementedError

    def verify(self, tx_id, device, desired_state):
        raise NotImplementedError

    def recover(self, tx_id, device):
        raise NotImplementedError

    def force_unlock(self, device, lock_owner, reason):
        raise NotImplementedError
