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
        )

    def Rollback(self, request, context):
        driver = self._registry.select(request.device)
        return driver.rollback(
            tx_id=request.context.tx_id if request.context else "",
            device=request.device,
        )

    def Verify(self, request, context):
        driver = self._registry.select(request.device)
        return driver.verify(
            tx_id=request.context.tx_id if request.context else "",
            device=request.device,
            desired_state=request.desired_state,
        )

    def Recover(self, request, context):
        driver = self._registry.select(request.device)
        try:
            return driver.recover(
                tx_id=request.context.tx_id if request.context else "",
                device=request.device,
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
) -> NetconfBackedDriver | AdapterErrorDriver:
    try:
        secret = secret_provider.resolve(device.secret_ref)
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
        )
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
