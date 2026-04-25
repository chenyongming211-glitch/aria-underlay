from __future__ import annotations

from concurrent import futures
from pathlib import Path
import sys

import grpc
import structlog

from aria_underlay_adapter.config import AdapterConfig
from aria_underlay_adapter.drivers.base import DriverRegistry
from aria_underlay_adapter.drivers.fake import FakeDriver

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


log = structlog.get_logger(__name__)


class UnderlayAdapterService(pb2_grpc.UnderlayAdapterServicer):
    def __init__(self, registry: DriverRegistry):
        self._registry = registry

    def GetCapabilities(self, request, context):
        driver = self._registry.select(request.device)
        return driver.get_capabilities(request)

    def GetCurrentState(self, request, context):
        return pb2.GetCurrentStateResponse(
            state=pb2.ObservedDeviceState(device_id=request.device.device_id)
        )

    def DryRun(self, request, context):
        return pb2.DryRunResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE,
                changed=False,
            )
        )

    def Prepare(self, request, context):
        return pb2.PrepareResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_PREPARED,
                changed=False,
            )
        )

    def Commit(self, request, context):
        return pb2.CommitResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                changed=False,
            )
        )

    def Rollback(self, request, context):
        return pb2.RollbackResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_ROLLED_BACK,
                changed=False,
            )
        )

    def Verify(self, request, context):
        return pb2.VerifyResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE,
                changed=False,
            )
        )

    def Recover(self, request, context):
        return pb2.RecoverResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE,
                changed=False,
            )
        )

    def ForceUnlock(self, request, context):
        if not request.break_glass_enabled:
            return pb2.ForceUnlockResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    warnings=["break-glass force unlock is disabled"],
                )
            )
        return pb2.ForceUnlockResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                changed=True,
            )
        )


def serve() -> None:
    config = AdapterConfig.from_env()
    registry = DriverRegistry(default_driver=FakeDriver())
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=8))
    pb2_grpc.add_UnderlayAdapterServicer_to_server(
        UnderlayAdapterService(registry),
        server,
    )
    server.add_insecure_port(config.listen)
    server.start()
    log.info("aria_underlay_adapter_started", listen=config.listen)
    server.wait_for_termination()


if __name__ == "__main__":
    serve()
