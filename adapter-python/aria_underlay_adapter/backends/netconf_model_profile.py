from __future__ import annotations

from aria_underlay_adapter.backends.base import BackendCapability
from aria_underlay_adapter.model_profile import (
    extract_yang_modules_from_capabilities,
    openconfig_netconf_probe_targets,
)


VALIDATE_11 = "urn:ietf:params:netconf:capability:validate:1.1"


def probe_openconfig_netconf_paths(
    session,
    capability: BackendCapability,
) -> dict[str, dict]:
    supported_modules = extract_yang_modules_from_capabilities(capability.raw_capabilities)
    supports_test_only = VALIDATE_11 in set(capability.raw_capabilities)
    verified_paths: dict[str, dict] = {}

    for target in openconfig_netconf_probe_targets(supported_modules):
        notes: list[str] = []
        readable = False
        writable = False

        try:
            session.get_config(
                source="running",
                filter=("subtree", target.read_filter_xml),
            )
            readable = True
            notes.append("netconf get-config read probe succeeded")
        except Exception as exc:
            notes.append(f"netconf get-config read probe failed: {_probe_error_summary(exc)}")

        if readable and capability.supports_candidate and supports_test_only:
            try:
                session.edit_config(
                    target="candidate",
                    config=target.test_config_xml,
                    default_operation="merge",
                    test_option="test-only",
                    error_option="rollback-on-error",
                )
                writable = True
                notes.append("netconf test-only write probe succeeded")
            except Exception as exc:
                notes.append(
                    f"netconf test-only write probe failed: {_probe_error_summary(exc)}"
                )
        elif not capability.supports_candidate or not capability.supports_validate:
            notes.append("netconf write probe skipped: missing candidate or validate support")
        elif readable:
            notes.append("missing safe NETCONF test-only support")

        verified_paths[target.path] = {
            "protocol": "openconfig_netconf",
            "model": target.model,
            "revision": target.revision,
            "readable": readable,
            "writable": writable,
            "deviations": [],
            "notes": notes,
        }

    return verified_paths


def _probe_error_summary(exc: Exception) -> str:
    summary = str(exc).strip()
    if summary:
        return summary
    return exc.__class__.__name__
