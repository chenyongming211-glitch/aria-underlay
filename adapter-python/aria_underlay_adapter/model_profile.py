from __future__ import annotations

from urllib.parse import parse_qs, urlparse

BGP_REQUIRED_MODULES = {
    "openconfig-network-instance",
    "openconfig-bgp",
    "openconfig-routing-policy",
}
BGP_REQUIRED_PATHS = {
    "/network-instances/network-instance/protocols/protocol/bgp",
    "/routing-policy",
}
PBR_REQUIRED_MODULES = {
    "openconfig-network-instance",
    "openconfig-policy-forwarding",
    "openconfig-acl",
    "openconfig-interfaces",
}
PBR_REQUIRED_PATHS = {
    "/network-instances/network-instance/policy-forwarding",
    "/interfaces",
}


def extract_yang_modules_from_capabilities(capabilities: list[str]) -> dict[str, str]:
    modules: dict[str, str] = {}
    for capability in capabilities:
        parsed = urlparse(capability)
        params = parse_qs(parsed.query)
        module = params.get("module", [None])[0]
        revision = params.get("revision", [""])[0]
        if module:
            modules[module] = revision
    return modules


def classify_model_profile(
    *,
    vendor: str,
    model: str,
    os_version: str,
    supports_candidate: bool,
    supports_validate: bool,
    supported_modules: dict[str, str],
    verified_paths: dict[str, dict[str, bool]],
) -> dict:
    rejection_reasons: list[str] = []
    bgp_ready = _classify_feature(
        feature="bgp",
        required_modules=BGP_REQUIRED_MODULES,
        required_paths=BGP_REQUIRED_PATHS,
        supports_candidate=supports_candidate,
        supports_validate=supports_validate,
        supported_modules=supported_modules,
        verified_paths=verified_paths,
        rejection_reasons=rejection_reasons,
    )
    pbr_ready = _classify_feature(
        feature="pbr",
        required_modules=PBR_REQUIRED_MODULES,
        required_paths=PBR_REQUIRED_PATHS,
        supports_candidate=supports_candidate,
        supports_validate=supports_validate,
        supported_modules=supported_modules,
        verified_paths=verified_paths,
        rejection_reasons=rejection_reasons,
    )
    return {
        "profile_id": f"{vendor}:{model}:{os_version}",
        "vendor": vendor,
        "model": model,
        "os_version": os_version,
        "paths": _profile_paths(supported_modules=supported_modules, verified_paths=verified_paths),
        "bgp_write_readiness": bgp_ready,
        "pbr_write_readiness": pbr_ready,
        "rejection_reasons": rejection_reasons,
    }


def _classify_feature(
    *,
    feature: str,
    required_modules: set[str],
    required_paths: set[str],
    supports_candidate: bool,
    supports_validate: bool,
    supported_modules: dict[str, str],
    verified_paths: dict[str, dict[str, bool]],
    rejection_reasons: list[str],
) -> str:
    if not supports_candidate or not supports_validate:
        rejection_reasons.append(f"{feature}: missing candidate or validate support")
        return "write_rejected"
    missing_modules = sorted(required_modules.difference(supported_modules.keys()))
    if missing_modules:
        rejection_reasons.append(f"{feature}: missing modules {', '.join(missing_modules)}")
        return "write_rejected"
    for path in sorted(required_paths):
        path_result = verified_paths.get(path)
        if not path_result:
            rejection_reasons.append(f"{feature}: missing verified path {path}")
            return "write_rejected"
        if not path_result.get("readable", False):
            rejection_reasons.append(f"{feature}: path is not readable {path}")
            return "write_rejected"
        if not path_result.get("writable", False):
            rejection_reasons.append(f"{feature}: path is read-only {path}")
            return "read_only"
    return "write_safe"


def _profile_paths(
    *,
    supported_modules: dict[str, str],
    verified_paths: dict[str, dict[str, bool]],
) -> list[dict]:
    paths: list[dict] = []
    for path, result in sorted(verified_paths.items()):
        paths.append(
            {
                "protocol": "openconfig_netconf",
                "model": _best_matching_model(path, supported_modules),
                "revision": "",
                "path": path,
                "readable": result.get("readable", False),
                "writable": result.get("writable", False),
                "verified_on_device": True,
                "deviations": [],
                "notes": [],
            }
        )
    return paths


def _best_matching_model(path: str, supported_modules: dict[str, str]) -> str:
    if "bgp" in path and "openconfig-bgp" in supported_modules:
        return "openconfig-bgp"
    if "policy-forwarding" in path and "openconfig-policy-forwarding" in supported_modules:
        return "openconfig-policy-forwarding"
    if "routing-policy" in path or path == "/routing-policy":
        return "openconfig-routing-policy"
    if "interfaces" in path and "openconfig-interfaces" in supported_modules:
        return "openconfig-interfaces"
    return ""
