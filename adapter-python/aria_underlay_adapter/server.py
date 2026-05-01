from __future__ import annotations

from concurrent import futures
from pathlib import Path
import sys

import grpc
import structlog

from aria_underlay_adapter.config import AdapterConfig
from aria_underlay_adapter.drivers.base import DriverRegistry

_PROTO_DIR = Path(__file__).resolve().parent / "proto"
if str(_PROTO_DIR) not in sys.path:
    sys.path.insert(0, str(_PROTO_DIR))

try:
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2_grpc as pb2_grpc
except ImportError as exc:  # pragma: no cover - exercised before proto generation
    raise SystemExit(
        "generated protobuf modules are missing; run grpcio-tools for "
        "proto/aria_underlay_adapter.proto before starting the adapter"
    ) from exc

from aria_underlay_adapter.drivers.fake import FakeDriver
from aria_underlay_adapter.backends.netconf import NcclientNetconfBackend
from aria_underlay_adapter.drivers.error import AdapterErrorDriver
from aria_underlay_adapter.drivers.netconf_backed import NetconfBackedDriver
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.secret_provider import LocalSecretProvider


log = structlog.get_logger(__name__)


class UnderlayAdapterService(pb2_grpc.UnderlayAdapterServicer):
    def __init__(self, registry: DriverRegistry):
        self._registry = registry

    def GetCapabilities(self, request, context):
        driver = self._registry.select(request.device)
        return driver.get_capabilities(request)

    def GetCurrentState(self, request, context):
        driver = self._registry.select(request.device)
        return driver.get_current_state(request)

    def DryRun(self, request, context):
        driver = self._registry.select(request.device)
        try:
            return driver.dry_run(
                device=request.device,
                desired_state=request.desired_state,
            )
        except AdapterError as error:
            return pb2.DryRunResponse(result=_failed_result(error))
        except NotImplementedError as error:
            return pb2.DryRunResponse(result=_not_implemented_result("dry_run", error))

    def Prepare(self, request, context):
        driver = self._registry.select(request.device)
        return driver.prepare(request)

    def Commit(self, request, context):
        driver = self._registry.select(request.device)
        return driver.commit(
            tx_id=request.context.tx_id if request.context else "",
            device=request.device,
            strategy=request.strategy,
            confirm_timeout_secs=request.confirm_timeout_secs,
        )

    def FinalConfirm(self, request, context):
        driver = self._registry.select(request.device)
        return driver.final_confirm(
            tx_id=request.context.tx_id if request.context else "",
            device=request.device,
        )

    def Rollback(self, request, context):
        driver = self._registry.select(request.device)
        return driver.rollback(
            tx_id=request.context.tx_id if request.context else "",
            device=request.device,
            strategy=request.strategy,
        )

    def Verify(self, request, context):
        driver = self._registry.select(request.device)
        return driver.verify(
            tx_id=request.context.tx_id if request.context else "",
            device=request.device,
            desired_state=request.desired_state,
            scope=request.scope if request.HasField("scope") else None,
        )

    def Recover(self, request, context):
        driver = self._registry.select(request.device)
        try:
            return driver.recover(
                tx_id=request.context.tx_id if request.context else "",
                device=request.device,
                strategy=request.strategy,
                action=request.action,
            )
        except AdapterError as error:
            return pb2.RecoverResponse(result=_failed_result(error))
        except NotImplementedError as error:
            return pb2.RecoverResponse(result=_not_implemented_result("recover", error))

    def ForceUnlock(self, request, context):
        if not request.break_glass_enabled:
            return pb2.ForceUnlockResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    warnings=["break-glass force unlock is disabled"],
                )
            )
        driver = self._registry.select(request.device)
        try:
            response = driver.force_unlock(
                device=request.device,
                lock_owner=request.lock_owner,
                reason=request.reason,
            )
        except AdapterError as error:
            return pb2.ForceUnlockResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    errors=[error.to_proto(pb2)],
                )
            )
        except NotImplementedError as error:
            return pb2.ForceUnlockResponse(
                result=_not_implemented_result("force_unlock", error)
            )
        if response is not None:
            return response
        return pb2.ForceUnlockResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                changed=True,
            )
        )


def serve() -> None:
    config = AdapterConfig.from_env()
    if config.fake_mode:
        registry = DriverRegistry(default_driver=FakeDriver(profile=config.fake_profile))
    else:
        secret_provider = LocalSecretProvider(secret_file=config.secret_file)
        registry = DriverRegistry(
            driver_factory=lambda device: _netconf_driver_from_device(
                device,
                secret_provider,
                tofu_known_hosts_path=config.tofu_known_hosts_file,
            )
        )
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=8))
    pb2_grpc.add_UnderlayAdapterServicer_to_server(
        UnderlayAdapterService(registry),
        server,
    )
    server.add_insecure_port(config.listen)
    server.start()
    log.info("aria_underlay_adapter_started", listen=config.listen)
    server.wait_for_termination()


def _netconf_driver_from_device(
    device,
    secret_provider: LocalSecretProvider,
    tofu_known_hosts_path: str | None = None,
) -> NetconfBackedDriver | AdapterErrorDriver:
    try:
        secret = secret_provider.resolve(device.secret_ref)
    except AdapterError as error:
        return AdapterErrorDriver(error)
    try:
        host_key_kwargs = _host_key_policy_kwargs(
            device,
            tofu_known_hosts_path=tofu_known_hosts_path,
        )
    except AdapterError as error:
        return AdapterErrorDriver(error)

    return NetconfBackedDriver(
        NcclientNetconfBackend(
            host=device.management_ip,
            port=device.management_port or 830,
            username=secret.username,
            password=secret.password,
            key_path=secret.key_path,
            passphrase=secret.passphrase,
            **host_key_kwargs,
        )
    )


def _host_key_policy_kwargs(device, *, tofu_known_hosts_path: str | None = None):
    policy = getattr(device, "host_key_policy", pb2.HOST_KEY_POLICY_UNSPECIFIED)

    if policy == pb2.HOST_KEY_POLICY_TRUST_ON_FIRST_USE:
        return {
            "hostkey_verify": True,
            "tofu_known_hosts_path": (
                tofu_known_hosts_path
                or "/tmp/aria-underlay-adapter/tofu_known_hosts"
            ),
        }

    if policy == pb2.HOST_KEY_POLICY_KNOWN_HOSTS_FILE:
        known_hosts_path = getattr(device, "known_hosts_path", "")
        if not known_hosts_path:
            raise AdapterError(
                code="HOST_KEY_POLICY_INVALID",
                message="known_hosts host key policy requires a path",
                normalized_error="known_hosts path missing",
                raw_error_summary="DeviceRef.known_hosts_path is empty",
                retryable=False,
            )
        return {
            "hostkey_verify": True,
            "known_hosts_path": known_hosts_path,
        }

    if policy == pb2.HOST_KEY_POLICY_PINNED_KEY:
        fingerprint = getattr(device, "pinned_host_key_fingerprint", "")
        if not fingerprint:
            raise AdapterError(
                code="HOST_KEY_POLICY_INVALID",
                message="pinned host key policy requires a fingerprint",
                normalized_error="pinned host key fingerprint missing",
                raw_error_summary="DeviceRef.pinned_host_key_fingerprint is empty",
                retryable=False,
            )
        return {
            "hostkey_verify": True,
            "pinned_host_key_fingerprint": fingerprint,
        }

    if policy == pb2.HOST_KEY_POLICY_UNSPECIFIED:
        raise AdapterError(
            code="HOST_KEY_POLICY_REQUIRED",
            message="host key policy is required for NETCONF devices",
            normalized_error="host key policy missing",
            raw_error_summary="DeviceRef.host_key_policy is HOST_KEY_POLICY_UNSPECIFIED",
            retryable=False,
        )

    raise AdapterError(
        code="HOST_KEY_POLICY_UNSUPPORTED",
        message="host key policy is unsupported",
        normalized_error="unsupported host key policy",
        raw_error_summary=f"DeviceRef.host_key_policy={policy!r}",
        retryable=False,
    )


def _failed_result(error: AdapterError):
    return pb2.AdapterResult(
        status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
        changed=False,
        errors=[error.to_proto(pb2)],
    )


def _not_implemented_result(operation: str, error: NotImplementedError):
    return _failed_result(
        AdapterError(
            code="NOT_IMPLEMENTED",
            message=f"{operation} is not implemented for selected driver",
            normalized_error="driver method missing",
            raw_error_summary=str(error) or operation,
            retryable=False,
        )
    )


if __name__ == "__main__":
    serve()
