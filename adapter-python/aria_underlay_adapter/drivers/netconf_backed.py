from __future__ import annotations

from aria_underlay_adapter.backends.base import NetconfBackend
from aria_underlay_adapter.errors import AdapterError

try:
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
except ImportError as exc:  # pragma: no cover
    raise RuntimeError("generated protobuf modules are missing") from exc


class NetconfBackedDriver:
    def __init__(self, backend: NetconfBackend):
        self._backend = backend

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

    def get_current_state(self, request):
        try:
            state = self._backend.get_current_state()
        except AdapterError as error:
            return pb2.GetCurrentStateResponse(errors=[error.to_proto(pb2)])

        return pb2.GetCurrentStateResponse(
            state=pb2.ObservedDeviceState(
                device_id=request.device.device_id,
                vlans=[
                    pb2.VlanConfig(
                        vlan_id=vlan["vlan_id"],
                        name=vlan["name"],
                        description=vlan["description"],
                    )
                    for vlan in state["vlans"]
                ],
                interfaces=[
                    pb2.InterfaceConfig(
                        name=iface["name"],
                        admin_state=pb2.ADMIN_STATE_UP
                        if iface["admin_state"] == "up"
                        else pb2.ADMIN_STATE_DOWN,
                        description=iface["description"],
                        mode=self._port_mode_to_proto(iface["mode"]),
                    )
                    for iface in state["interfaces"]
                ],
            )
        )

    def dry_run(self, device, desired_state):
        raise NotImplementedError

    def prepare(self, request):
        try:
            self._backend.prepare_candidate()
        except AdapterError as error:
            return pb2.PrepareResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    errors=[error.to_proto(pb2)],
                )
            )

        return pb2.PrepareResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_PREPARED,
                changed=True,
            )
        )

    def commit(self, tx_id, device):
        try:
            self._backend.commit_candidate()
        except AdapterError as error:
            return pb2.CommitResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    errors=[error.to_proto(pb2)],
                )
            )

        return pb2.CommitResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                changed=True,
            )
        )

    def rollback(self, tx_id, device):
        try:
            self._backend.rollback_candidate()
        except AdapterError as error:
            return pb2.RollbackResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    errors=[error.to_proto(pb2)],
                )
            )

        return pb2.RollbackResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_ROLLED_BACK,
                changed=True,
            )
        )

    def verify(self, tx_id, device, desired_state):
        try:
            self._backend.verify_running(desired_state)
        except AdapterError as error:
            return pb2.VerifyResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    errors=[error.to_proto(pb2)],
                )
            )

        return pb2.VerifyResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE,
                changed=False,
            )
        )

    def recover(self, tx_id, device):
        raise NotImplementedError

    def force_unlock(self, device, lock_owner, reason):
        raise AdapterError(
            code="NOT_IMPLEMENTED",
            message="force unlock is not implemented for NETCONF backend",
            normalized_error="force unlock operation missing",
            raw_error_summary="NETCONF kill-session support is not implemented yet",
            retryable=False,
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

    def _port_mode_to_proto(self, mode: dict):
        if mode["kind"] == "trunk":
            return pb2.PortMode(
                kind=pb2.PORT_MODE_KIND_TRUNK,
                native_vlan=mode["native_vlan"],
                allowed_vlans=mode["allowed_vlans"],
            )
        if mode["kind"] == "access":
            return pb2.PortMode(
                kind=pb2.PORT_MODE_KIND_ACCESS,
                access_vlan=mode["access_vlan"],
                allowed_vlans=mode["allowed_vlans"],
            )

        raise AdapterError(
            code="INVALID_PORT_MODE",
            message=f"unknown port mode kind: {mode['kind']}",
        )
