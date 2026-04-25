from __future__ import annotations

from aria_underlay_adapter.drivers.base import DeviceDriver

try:
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
except ImportError as exc:  # pragma: no cover
    raise RuntimeError("generated protobuf modules are missing") from exc


class FakeDriver(DeviceDriver):
    def get_capabilities(self, request):
        return pb2.GetCapabilitiesResponse(
            capability=pb2.DeviceCapability(
                vendor=pb2.VENDOR_UNKNOWN,
                model="fake-switch",
                os_version="fake-0.1",
                raw_capabilities=[
                    "urn:ietf:params:netconf:base:1.0",
                    "urn:ietf:params:netconf:capability:candidate:1.0",
                    "urn:ietf:params:netconf:capability:validate:1.1",
                    "urn:ietf:params:netconf:capability:confirmed-commit:1.1",
                ],
                supports_netconf=True,
                supports_candidate=True,
                supports_validate=True,
                supports_confirmed_commit=True,
                supports_persist_id=True,
                supports_rollback_on_error=False,
                supports_writable_running=False,
                supported_backends=[pb2.BACKEND_KIND_NETCONF],
            )
        )

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

