from __future__ import annotations

import copy
from dataclasses import replace

from aria_underlay_adapter.backends.base import NetconfBackend
from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.registry import renderer_for_vendor
from aria_underlay_adapter.state_parsers.registry import state_parser_for_vendor

try:
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2 as pb2
except ImportError as exc:  # pragma: no cover
    raise RuntimeError("generated protobuf modules are missing") from exc


class NetconfBackedDriver:
    def __init__(
        self,
        backend: NetconfBackend,
        *,
        allow_fixture_verified_parser: bool = False,
    ):
        self._backend = backend
        self._allow_fixture_verified_parser = allow_fixture_verified_parser

    def get_capabilities(self, request):
        try:
            capability = self._backend.get_capabilities()
        except AdapterError as error:
            return pb2.GetCapabilitiesResponse(errors=[error.to_proto(pb2)])
        except Exception as exc:
            return pb2.GetCapabilitiesResponse(errors=[_unexpected_error(exc).to_proto(pb2)])

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
            backend = self._backend_for_state_read(request.device)
            state = backend.get_current_state(scope=_request_scope(request))
            observed_state = self._observed_state_to_proto(
                request.device.device_id,
                state,
            )
        except AdapterError as error:
            return pb2.GetCurrentStateResponse(errors=[error.to_proto(pb2)])
        except Exception as exc:
            return pb2.GetCurrentStateResponse(errors=[_unexpected_error(exc).to_proto(pb2)])

        return pb2.GetCurrentStateResponse(state=observed_state)

    def dry_run(self, device, desired_state):
        try:
            backend = self._backend_for_dry_run(device, desired_state)
            result = backend.dry_run_candidate(desired_state)
        except AdapterError as error:
            return pb2.DryRunResponse(result=_failed_result(error))
        except AttributeError as exc:
            error = AdapterError(
                code="DRY_RUN_NOT_SUPPORTED",
                message="selected NETCONF backend does not implement dry-run",
                normalized_error="backend dry-run missing",
                raw_error_summary=str(exc),
                retryable=False,
            )
            return pb2.DryRunResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.DryRunResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.DryRunResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE,
                changed=result.changed,
                warnings=list(result.warnings),
            )
        )

    def prepare(self, request):
        try:
            backend = self._backend_for_prepare(request)
            backend.prepare_candidate(getattr(request, "desired_state", None))
        except AdapterError as error:
            return pb2.PrepareResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.PrepareResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.PrepareResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_PREPARED,
                changed=True,
            )
        )

    def _backend_for_prepare(self, request):
        if not hasattr(self._backend, "config_renderer"):
            return self._backend
        if getattr(self._backend, "config_renderer", None) is not None:
            return self._backend

        renderer = renderer_for_vendor(request.device.vendor_hint)
        return self._replace_backend(config_renderer=renderer)

    def _backend_for_dry_run(self, device, desired_state):
        if not hasattr(self._backend, "config_renderer"):
            return self._backend
        if getattr(self._backend, "config_renderer", None) is not None:
            return self._backend
        if _desired_state_is_empty(desired_state):
            return self._backend

        renderer = renderer_for_vendor(device.vendor_hint)
        return self._replace_backend(config_renderer=renderer)

    def _backend_for_state_read(self, device):
        if not hasattr(self._backend, "state_parser"):
            return self._backend
        if getattr(self._backend, "state_parser", None) is not None:
            return self._backend

        parser = state_parser_for_vendor(
            device.vendor_hint,
            allow_fixture_verified=self._allow_fixture_verified_parser,
            model_hint=getattr(device, "model_hint", ""),
        )
        return self._replace_backend(
            state_parser=parser,
            allow_fixture_verified_state_parser=self._allow_fixture_verified_parser,
        )

    def _replace_backend(self, **changes):
        try:
            return replace(self._backend, **changes)
        except TypeError:
            backend = copy.copy(self._backend)
            for name, value in changes.items():
                object.__setattr__(backend, name, value)
            return backend

    def commit(self, tx_id, device, strategy=None, confirm_timeout_secs=120):
        try:
            self._backend.commit_candidate(
                strategy=strategy,
                tx_id=tx_id,
                confirm_timeout_secs=confirm_timeout_secs or 120,
            )
        except AdapterError as error:
            return pb2.CommitResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.CommitResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.CommitResponse(
            result=pb2.AdapterResult(
                status=(
                    pb2.ADAPTER_OPERATION_STATUS_CONFIRMED_COMMIT_PENDING
                    if strategy == pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT
                    else pb2.ADAPTER_OPERATION_STATUS_COMMITTED
                ),
                changed=True,
            )
        )

    def final_confirm(self, tx_id, device):
        try:
            self._backend.final_confirm(tx_id=tx_id)
        except AdapterError as error:
            return pb2.FinalConfirmResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.FinalConfirmResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.FinalConfirmResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                changed=True,
            )
        )

    def rollback(self, tx_id, device, strategy=None):
        try:
            self._backend.rollback_candidate(strategy=strategy, tx_id=tx_id)
        except AdapterError as error:
            return pb2.RollbackResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.RollbackResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.RollbackResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_ROLLED_BACK,
                changed=True,
            )
        )

    def verify(self, tx_id, device, desired_state, scope=None):
        try:
            backend = self._backend_for_state_read(device)
            backend.verify_running(desired_state, scope=_message_or_none(scope))
        except AdapterError as error:
            return pb2.VerifyResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.VerifyResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.VerifyResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_NO_CHANGE,
                changed=False,
            )
        )

    def recover(self, tx_id, device, strategy=None, action=None):
        if action == pb2.RECOVERY_ACTION_DISCARD_PREPARED_CHANGES:
            rollback_strategy = strategy
        elif action == pb2.RECOVERY_ACTION_ADAPTER_RECOVER:
            rollback_strategy = strategy
        else:
            return pb2.RecoverResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
                    changed=False,
                    errors=[
                        AdapterError(
                            code="RECOVERY_ACTION_UNSUPPORTED",
                            message="recover request action is unsupported",
                            normalized_error="unsupported recovery action",
                            raw_error_summary=f"action={action!r}, strategy={strategy!r}",
                            retryable=False,
                        ).to_proto(pb2)
                    ],
                )
            )

        if (
            action == pb2.RECOVERY_ACTION_ADAPTER_RECOVER
            and strategy == pb2.TRANSACTION_STRATEGY_CANDIDATE_COMMIT
        ):
            return pb2.RecoverResponse(
                result=pb2.AdapterResult(
                    status=pb2.ADAPTER_OPERATION_STATUS_IN_DOUBT,
                    changed=False,
                    errors=[
                        AdapterError(
                            code="CANDIDATE_COMMIT_RECOVERY_IN_DOUBT",
                            message=(
                                "candidate commit recovery cannot prove whether "
                                "running config already changed"
                            ),
                            normalized_error="candidate commit recovery in doubt",
                            raw_error_summary=f"tx_id={tx_id or ''}",
                            retryable=False,
                        ).to_proto(pb2)
                    ],
                )
            )

        if (
            action == pb2.RECOVERY_ACTION_ADAPTER_RECOVER
            and strategy == pb2.TRANSACTION_STRATEGY_CONFIRMED_COMMIT
        ):
            try:
                self._backend.final_confirm(tx_id=tx_id)
            except AdapterError as error:
                if _persist_id_already_consumed(error):
                    return _recover_response(
                        pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                        changed=True,
                    )
            except Exception as exc:
                return pb2.RecoverResponse(result=_failed_result(_unexpected_error(exc)))
            else:
                return _recover_response(
                    pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                    changed=True,
                )

        try:
            self._backend.rollback_candidate(strategy=rollback_strategy, tx_id=tx_id)
        except AdapterError as error:
            return pb2.RecoverResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.RecoverResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.RecoverResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_ROLLED_BACK,
                changed=True,
            )
        )

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

    def _observed_state_to_proto(self, device_id: str, state: dict):
        if not isinstance(state, dict):
            raise _parsed_state_error(f"parsed_state_type={type(state).__name__}")

        vlans = state.get("vlans", [])
        interfaces = state.get("interfaces", [])
        if not isinstance(vlans, list):
            raise _parsed_state_error(f"vlans must be a list, got {type(vlans).__name__}")
        if not isinstance(interfaces, list):
            raise _parsed_state_error(
                f"interfaces must be a list, got {type(interfaces).__name__}"
            )

        return pb2.ObservedDeviceState(
            device_id=device_id,
            vlans=[
                self._vlan_to_proto(vlan, index)
                for index, vlan in enumerate(vlans)
            ],
            interfaces=[
                self._interface_to_proto(interface, index)
                for index, interface in enumerate(interfaces)
            ],
        )

    def _vlan_to_proto(self, vlan: dict, index: int):
        if not isinstance(vlan, dict):
            raise _parsed_state_error(
                f"vlans[{index}] must be an object, got {type(vlan).__name__}"
            )

        vlan_id = _parsed_vlan_id(vlan.get("vlan_id"), f"vlans[{index}].vlan_id")
        return pb2.VlanConfig(
            vlan_id=vlan_id,
            name=vlan.get("name"),
            description=vlan.get("description"),
        )

    def _interface_to_proto(self, interface: dict, index: int):
        if not isinstance(interface, dict):
            raise _parsed_state_error(
                "interfaces[{}] must be an object, got {}".format(
                    index,
                    type(interface).__name__,
                )
            )

        name = interface.get("name")
        if not isinstance(name, str) or not name.strip():
            raise _parsed_state_error(f"interfaces[{index}].name must be non-empty")

        return pb2.InterfaceConfig(
            name=name,
            admin_state=_admin_state_to_proto(
                interface.get("admin_state"),
                f"interfaces[{index}].admin_state",
            ),
            description=interface.get("description"),
            mode=self._port_mode_to_proto(
                interface.get("mode"),
                path=f"interfaces[{index}].mode",
            ),
        )

    def _port_mode_to_proto(self, mode: dict, *, path: str = "mode"):
        if not isinstance(mode, dict):
            raise _parsed_state_error(f"{path} must be an object")

        raw_kind = mode.get("kind")
        kind = raw_kind.strip().lower() if isinstance(raw_kind, str) else raw_kind
        if kind == "trunk":
            native_vlan = _optional_parsed_vlan_id(
                mode.get("native_vlan"),
                f"{path}.native_vlan",
            )
            allowed_vlans = [
                _parsed_vlan_id(vlan_id, f"{path}.allowed_vlans[{index}]")
                for index, vlan_id in enumerate(mode.get("allowed_vlans", []))
            ]
            kwargs = {
                "kind": pb2.PORT_MODE_KIND_TRUNK,
                "allowed_vlans": allowed_vlans,
            }
            if native_vlan is not None:
                kwargs["native_vlan"] = native_vlan
            return pb2.PortMode(**kwargs)
        if kind == "access":
            access_vlan = _parsed_vlan_id(mode.get("access_vlan"), f"{path}.access_vlan")
            return pb2.PortMode(
                kind=pb2.PORT_MODE_KIND_ACCESS,
                access_vlan=access_vlan,
            )

        if path == "mode":
            raise AdapterError(
                code="INVALID_PORT_MODE",
                message=f"unknown port mode kind: {raw_kind}",
            )
        raise _parsed_state_error(f"{path}.kind has unsupported value: {raw_kind!r}")


def _request_scope(request):
    if hasattr(request, "HasField"):
        try:
            if request.HasField("scope"):
                return request.scope
        except ValueError:
            return None
        return None
    return getattr(request, "scope", None)


def _message_or_none(message):
    return message if message is not None else None


def _desired_state_is_empty(desired_state) -> bool:
    return (
        desired_state is None
        or (
            not list(getattr(desired_state, "vlans", []))
            and not list(getattr(desired_state, "interfaces", []))
        )
    )


def _admin_state_to_proto(value, path: str):
    if value is None or value == "" or value == 0:
        return pb2.ADMIN_STATE_UP
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized == "up":
            return pb2.ADMIN_STATE_UP
        if normalized == "down":
            return pb2.ADMIN_STATE_DOWN
        raise _parsed_state_error(f"{path} has unsupported value: {value!r}")
    try:
        numeric = int(value)
    except (TypeError, ValueError) as exc:
        raise _parsed_state_error(f"{path} must be up/down or enum value") from exc
    if numeric == pb2.ADMIN_STATE_UP:
        return pb2.ADMIN_STATE_UP
    if numeric == pb2.ADMIN_STATE_DOWN:
        return pb2.ADMIN_STATE_DOWN
    raise _parsed_state_error(f"{path} has unsupported value: {value!r}")


def _optional_parsed_vlan_id(value, path: str):
    if value is None or value == "":
        return None
    return _parsed_vlan_id(value, path)


def _parsed_vlan_id(value, path: str) -> int:
    try:
        vlan_id = int(value)
    except (TypeError, ValueError) as exc:
        raise _parsed_state_error(f"{path} must be an integer: {value!r}") from exc
    if vlan_id < 1 or vlan_id > 4094:
        raise _parsed_state_error(f"{path} out of range: {vlan_id}")
    return vlan_id


def _parsed_state_error(summary: str) -> AdapterError:
    return AdapterError(
        code="NETCONF_STATE_PARSE_FAILED",
        message="NETCONF running state parser returned invalid state",
        normalized_error="invalid parsed state",
        raw_error_summary=summary,
        retryable=False,
    )


def _failed_result(error: AdapterError):
    return pb2.AdapterResult(
        status=pb2.ADAPTER_OPERATION_STATUS_FAILED,
        changed=False,
        errors=[error.to_proto(pb2)],
    )


def _recover_response(status, *, changed: bool):
    return pb2.RecoverResponse(
        result=pb2.AdapterResult(
            status=status,
            changed=changed,
        )
    )


def _unexpected_error(exc: Exception) -> AdapterError:
    return AdapterError(
        code="ADAPTER_INTERNAL_ERROR",
        message="adapter operation raised an unexpected exception",
        normalized_error="unexpected adapter exception",
        raw_error_summary=f"{type(exc).__name__}: {exc}",
        retryable=False,
    )


def _persist_id_already_consumed(error: AdapterError) -> bool:
    if error.code in {
        "NETCONF_PERSIST_ID_ALREADY_CONSUMED",
        "NETCONF_PERSIST_ID_NOT_FOUND",
    }:
        return True
    if error.normalized_error in {
        "persist-id already consumed",
        "persist-id not found",
        "unknown persist-id",
    }:
        return True

    text = " ".join(
        part
        for part in [
            error.code,
            error.message,
            error.normalized_error,
            error.raw_error_summary,
        ]
        if part
    ).lower()
    return "persist" in text and any(
        marker in text for marker in ["unknown", "not found", "consumed"]
    )
