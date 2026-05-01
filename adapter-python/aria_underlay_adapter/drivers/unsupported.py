from __future__ import annotations


class UnsupportedDriver:
    vendor_name = "Unsupported"

    def __init__(self, *args, **kwargs):
        pass

    def _unsupported(self, *args, **kwargs):
        raise NotImplementedError(f"{self.vendor_name}Driver is not implemented")

    def get_capabilities(self, request):
        self._unsupported(request)

    def get_current_state(self, request):
        self._unsupported(request)

    def dry_run(self, device, desired_state):
        self._unsupported(device, desired_state)

    def prepare(self, request):
        self._unsupported(request)

    def commit(self, tx_id, device, strategy=None, confirm_timeout_secs=120):
        self._unsupported(tx_id, device, strategy, confirm_timeout_secs)

    def final_confirm(self, tx_id, device):
        self._unsupported(tx_id, device)

    def rollback(self, tx_id, device, strategy=None):
        self._unsupported(tx_id, device, strategy)

    def verify(self, tx_id, device, desired_state, scope=None):
        self._unsupported(tx_id, device, desired_state, scope)

    def recover(self, tx_id, device, strategy=None, action=None):
        self._unsupported(tx_id, device, strategy, action)

    def force_unlock(self, device, lock_owner, reason):
        self._unsupported(device, lock_owner, reason)
