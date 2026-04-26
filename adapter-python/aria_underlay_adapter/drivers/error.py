from __future__ import annotations

from aria_underlay_adapter.errors import AdapterError

try:
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
except ImportError as exc:  # pragma: no cover
    raise RuntimeError("generated protobuf modules are missing") from exc


class AdapterErrorDriver:
    def __init__(self, error: AdapterError):
        self._error = error

    def get_capabilities(self, request):
        return pb2.GetCapabilitiesResponse(errors=[self._error.to_proto(pb2)])

    def get_current_state(self, request):
        return pb2.GetCurrentStateResponse(errors=[self._error.to_proto(pb2)])

    def dry_run(self, device, desired_state):
        return pb2.DryRunResponse(result=self._failed_result())

    def prepare(self, request):
        return pb2.PrepareResponse(result=self._failed_result())

    def commit(self, tx_id, device, strategy=None, confirm_timeout_secs=120):
        return pb2.CommitResponse(result=self._failed_result())

    def final_confirm(self, tx_id, device):
        return pb2.FinalConfirmResponse(result=self._failed_result())

    def rollback(self, tx_id, device, strategy=None):
        return pb2.RollbackResponse(result=self._failed_result())

    def verify(self, tx_id, device, desired_state, scope=None):
        return pb2.VerifyResponse(result=self._failed_result())

    def recover(self, tx_id, device):
        return pb2.RecoverResponse(result=self._failed_result())

    def force_unlock(self, device, lock_owner, reason):
        return pb2.ForceUnlockResponse(result=self._failed_result())

    def _failed_result(self):
        return pb2.AdapterResult(
            status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
            changed=False,
            errors=[self._error.to_proto(pb2)],
        )
