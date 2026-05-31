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

        capability_fields = {
            "vendor": pb2.VENDOR_UNKNOWN,
            "model": capability.model,
            "os_version": capability.os_version,
            "raw_capabilities": capability.raw_capabilities,
            "supports_netconf": capability.supports_netconf,
            "supports_candidate": capability.supports_candidate,
            "supports_validate": capability.supports_validate,
            "supports_confirmed_commit": capability.supports_confirmed_commit,
            "supports_persist_id": capability.supports_persist_id,
            "supports_rollback_on_error": capability.supports_rollback_on_error,
            "supports_writable_running": capability.supports_writable_running,
            "supported_backends": [
                self._backend_kind_to_proto(backend)
                for backend in capability.supported_backends
            ],
        }
        if capability.model_profile is not None:
            capability_fields["model_profile"] = _model_profile_to_proto(
                capability.model_profile
            )
        if capability.yang_modules:
            capability_fields["yang_modules"] = [
                _yang_module_to_proto(module) for module in capability.yang_modules
            ]

        return pb2.GetCapabilitiesResponse(
            capability=pb2.DeviceCapability(**capability_fields),
            warnings=list(capability.warnings),
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
            prepared = backend.prepare_candidate(getattr(request, "desired_state", None))
        except AdapterError as error:
            return pb2.PrepareResponse(result=_failed_result(error))
        except Exception as exc:
            return pb2.PrepareResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.PrepareResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_PREPARED,
                changed=True,
                prepared_candidate_checksum=getattr(
                    prepared,
                    "candidate_checksum",
                    "",
                ),
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

    def commit(
        self,
        tx_id,
        device,
        strategy=None,
        confirm_timeout_secs=120,
        prepared_candidate_checksum: str | None = None,
    ):
        try:
            result = self._backend.commit_candidate(
                strategy=strategy,
                tx_id=tx_id,
                confirm_timeout_secs=confirm_timeout_secs or 120,
                prepared_candidate_checksum=prepared_candidate_checksum,
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
                warnings=list(getattr(result, "warnings", [])),
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
        try:
            self._backend.force_unlock(lock_owner=lock_owner, reason=reason)
        except AdapterError as error:
            return pb2.ForceUnlockResponse(result=_failed_result(error))
        except AttributeError as exc:
            return pb2.ForceUnlockResponse(
                result=_failed_result(
                    AdapterError(
                        code="NOT_IMPLEMENTED",
                        message="force unlock is not implemented for selected NETCONF backend",
                        normalized_error="force unlock operation missing",
                        raw_error_summary=str(exc),
                        retryable=False,
                    )
                )
            )
        except Exception as exc:
            return pb2.ForceUnlockResponse(result=_failed_result(_unexpected_error(exc)))

        return pb2.ForceUnlockResponse(
            result=pb2.AdapterResult(
                status=pb2.ADAPTER_OPERATION_STATUS_COMMITTED,
                changed=True,
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

    def _observed_state_to_proto(self, device_id: str, state: dict):
        if not isinstance(state, dict):
            raise _parsed_state_error(f"parsed_state_type={type(state).__name__}")

        vlans = state.get("vlans", [])
        interfaces = state.get("interfaces", [])
        acls = state.get("acls", [])
        acl_bindings = state.get("acl_bindings", [])
        if not isinstance(vlans, list):
            raise _parsed_state_error(f"vlans must be a list, got {type(vlans).__name__}")
        if not isinstance(interfaces, list):
            raise _parsed_state_error(
                f"interfaces must be a list, got {type(interfaces).__name__}"
            )
        if not isinstance(acls, list):
            raise _parsed_state_error(f"acls must be a list, got {type(acls).__name__}")
        if not isinstance(acl_bindings, list):
            raise _parsed_state_error(
                f"acl_bindings must be a list, got {type(acl_bindings).__name__}"
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
            acls=[
                self._acl_to_proto(acl, index)
                for index, acl in enumerate(acls)
            ],
            acl_bindings=[
                self._acl_binding_to_proto(binding, index)
                for index, binding in enumerate(acl_bindings)
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

    def _acl_to_proto(self, acl: dict, index: int):
        if not isinstance(acl, dict):
            raise _parsed_state_error(
                f"acls[{index}] must be an object, got {type(acl).__name__}"
            )
        acl_id = _parsed_acl_id(acl.get("acl_id"), f"acls[{index}].acl_id")
        rules = acl.get("rules", [])
        if not isinstance(rules, list):
            raise _parsed_state_error(f"acls[{index}].rules must be a list")
        kwargs = {
            "acl_id": acl_id,
            "rules": [
                self._acl_rule_to_proto(rule, rule_index, path=f"acls[{index}].rules")
                for rule_index, rule in enumerate(rules)
            ],
        }
        if acl.get("name") is not None:
            kwargs["name"] = acl.get("name")
        if acl.get("description") is not None:
            kwargs["description"] = acl.get("description")
        return pb2.AclConfig(**kwargs)

    def _acl_binding_to_proto(self, binding: dict, index: int):
        if not isinstance(binding, dict):
            raise _parsed_state_error(
                f"acl_bindings[{index}] must be an object, got {type(binding).__name__}"
            )
        interface_name = binding.get("interface_name")
        if not isinstance(interface_name, str) or not interface_name.strip():
            raise _parsed_state_error(f"acl_bindings[{index}].interface_name must be non-empty")
        return pb2.AclBinding(
            interface_name=interface_name,
            direction=_acl_direction_to_proto(
                binding.get("direction"),
                f"acl_bindings[{index}].direction",
            ),
            acl_id=_parsed_acl_id(binding.get("acl_id"), f"acl_bindings[{index}].acl_id"),
        )

    def _acl_rule_to_proto(self, rule: dict, index: int, *, path: str):
        if not isinstance(rule, dict):
            raise _parsed_state_error(
                f"{path}[{index}] must be an object, got {type(rule).__name__}"
            )
        kwargs = {
            "sequence": _parsed_rule_sequence(rule.get("sequence"), f"{path}[{index}].sequence"),
            "action": _acl_action_to_proto(rule.get("action"), f"{path}[{index}].action"),
            "protocol": _acl_protocol_to_proto(rule.get("protocol"), f"{path}[{index}].protocol"),
        }
        source = _acl_endpoint_to_proto(rule.get("source"), f"{path}[{index}].source")
        if source is not None:
            kwargs["source"] = source
        destination = _acl_endpoint_to_proto(
            rule.get("destination"),
            f"{path}[{index}].destination",
        )
        if destination is not None:
            kwargs["destination"] = destination
        source_port = _optional_parsed_acl_port(
            rule.get("source_port_eq"),
            f"{path}[{index}].source_port_eq",
        )
        if source_port is not None:
            kwargs["source_port_eq"] = source_port
        destination_port = _optional_parsed_acl_port(
            rule.get("destination_port_eq"),
            f"{path}[{index}].destination_port_eq",
        )
        if destination_port is not None:
            kwargs["destination_port_eq"] = destination_port
        if rule.get("description") is not None:
            kwargs["description"] = rule.get("description")
        return pb2.AclRule(**kwargs)


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
            and not list(getattr(desired_state, "acls", []))
            and not list(getattr(desired_state, "acl_bindings", []))
            and not list(getattr(desired_state, "delete_vlan_ids", []))
            and not list(getattr(desired_state, "delete_acl_ids", []))
            and not list(getattr(desired_state, "delete_acl_bindings", []))
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


def _parsed_acl_id(value, path: str) -> int:
    try:
        acl_id = int(value)
    except (TypeError, ValueError) as exc:
        raise _parsed_state_error(f"{path} must be an integer: {value!r}") from exc
    if acl_id < 3000 or acl_id > 3999:
        raise _parsed_state_error(f"{path} out of IPv4 advanced ACL range: {acl_id}")
    return acl_id


def _parsed_rule_sequence(value, path: str) -> int:
    try:
        sequence = int(value)
    except (TypeError, ValueError) as exc:
        raise _parsed_state_error(f"{path} must be an integer: {value!r}") from exc
    if sequence < 0 or sequence > 65535:
        raise _parsed_state_error(f"{path} out of range: {sequence}")
    return sequence


def _optional_parsed_acl_port(value, path: str):
    if value is None or value == "":
        return None
    try:
        port = int(value)
    except (TypeError, ValueError) as exc:
        raise _parsed_state_error(f"{path} must be an integer: {value!r}") from exc
    if port < 1 or port > 65535:
        raise _parsed_state_error(f"{path} out of range: {port}")
    return port


def _acl_action_to_proto(value, path: str):
    normalized = value.strip().lower() if isinstance(value, str) else value
    if normalized == "permit" or normalized == pb2.ACL_ACTION_PERMIT:
        return pb2.ACL_ACTION_PERMIT
    if normalized == "deny" or normalized == pb2.ACL_ACTION_DENY:
        return pb2.ACL_ACTION_DENY
    raise _parsed_state_error(f"{path} has unsupported value: {value!r}")


def _acl_protocol_to_proto(value, path: str):
    normalized = value.strip().lower() if isinstance(value, str) else value
    if normalized == "ip" or normalized == pb2.ACL_PROTOCOL_IP:
        return pb2.ACL_PROTOCOL_IP
    if normalized == "tcp" or normalized == pb2.ACL_PROTOCOL_TCP:
        return pb2.ACL_PROTOCOL_TCP
    if normalized == "udp" or normalized == pb2.ACL_PROTOCOL_UDP:
        return pb2.ACL_PROTOCOL_UDP
    if normalized == "icmp" or normalized == pb2.ACL_PROTOCOL_ICMP:
        return pb2.ACL_PROTOCOL_ICMP
    raise _parsed_state_error(f"{path} has unsupported value: {value!r}")


def _acl_direction_to_proto(value, path: str):
    normalized = value.strip().lower() if isinstance(value, str) else value
    if normalized == "inbound" or normalized == pb2.ACL_DIRECTION_INBOUND:
        return pb2.ACL_DIRECTION_INBOUND
    if normalized == "outbound" or normalized == pb2.ACL_DIRECTION_OUTBOUND:
        return pb2.ACL_DIRECTION_OUTBOUND
    raise _parsed_state_error(f"{path} has unsupported value: {value!r}")


def _acl_endpoint_to_proto(value, path: str):
    if value in (None, ""):
        return None
    if not isinstance(value, dict):
        raise _parsed_state_error(f"{path} must be an object")
    address = value.get("address")
    wildcard = value.get("wildcard")
    if not isinstance(address, str) or not address.strip():
        raise _parsed_state_error(f"{path}.address must be non-empty")
    if not isinstance(wildcard, str) or not wildcard.strip():
        raise _parsed_state_error(f"{path}.wildcard must be non-empty")
    return pb2.AclEndpoint(address=address, wildcard=wildcard)


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


def _model_profile_to_proto(profile: dict):
    return pb2.DeviceModelProfile(
        profile_id=profile.get("profile_id", ""),
        vendor=_vendor_to_proto(profile.get("vendor", "unknown")),
        model=profile.get("model", ""),
        os_version=profile.get("os_version", ""),
        paths=[_model_path_to_proto(path) for path in profile.get("paths", [])],
        pbr_write_readiness=_write_readiness_to_proto(
            profile.get("pbr_write_readiness", "write_rejected")
        ),
        bgp_write_readiness=_write_readiness_to_proto(
            profile.get("bgp_write_readiness", "write_rejected")
        ),
        rejection_reasons=profile.get("rejection_reasons", []),
        yang_module_count=profile.get("yang_module_count", 0),
    )


def _yang_module_to_proto(module: dict):
    return pb2.YangModuleSummary(
        name=module.get("name", ""),
        revision=module.get("revision", ""),
        namespace=module.get("namespace", ""),
        schema_size_bytes=module.get("schema_size_bytes", 0),
        schema_downloaded=module.get("schema_downloaded", False),
        format=module.get("format", "yang"),
    )


def _model_path_to_proto(path: dict):
    return pb2.ModelPathSupport(
        protocol=_model_protocol_to_proto(path.get("protocol", "")),
        model=path.get("model", ""),
        revision=path.get("revision", ""),
        path=path.get("path", ""),
        readable=path.get("readable", False),
        writable=path.get("writable", False),
        verified_on_device=path.get("verified_on_device", False),
        deviations=path.get("deviations", []),
        notes=path.get("notes", []),
    )


def _model_protocol_to_proto(value: str):
    return {
        "openconfig_gnmi": pb2.MODEL_PROTOCOL_OPENCONFIG_GNMI,
        "openconfig_netconf": pb2.MODEL_PROTOCOL_OPENCONFIG_NETCONF,
        "vendor_native_yang": pb2.MODEL_PROTOCOL_VENDOR_NATIVE_YANG,
        "vendor_cli": pb2.MODEL_PROTOCOL_VENDOR_CLI,
    }.get(value, pb2.MODEL_PROTOCOL_UNSPECIFIED)


def _write_readiness_to_proto(value: str):
    return {
        "read_only": pb2.WRITE_READINESS_READ_ONLY,
        "write_safe": pb2.WRITE_READINESS_WRITE_SAFE,
        "write_rejected": pb2.WRITE_READINESS_WRITE_REJECTED,
    }.get(value, pb2.WRITE_READINESS_UNSPECIFIED)


def _vendor_to_proto(value: str):
    return {
        "huawei": pb2.VENDOR_HUAWEI,
        "h3c": pb2.VENDOR_H3C,
        "cisco": pb2.VENDOR_CISCO,
        "ruijie": pb2.VENDOR_RUIJIE,
        "unknown": pb2.VENDOR_UNKNOWN,
    }.get(value, pb2.VENDOR_UNKNOWN)


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
    return False
