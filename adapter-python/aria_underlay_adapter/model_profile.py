from __future__ import annotations

from dataclasses import dataclass
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
OPENCONFIG_NETWORK_INSTANCE_NS = "http://openconfig.net/yang/network-instance"
OPENCONFIG_BGP_NS = "http://openconfig.net/yang/bgp"
OPENCONFIG_ROUTING_POLICY_NS = "http://openconfig.net/yang/routing-policy"
OPENCONFIG_POLICY_FORWARDING_NS = "http://openconfig.net/yang/policy-forwarding"
OPENCONFIG_INTERFACES_NS = "http://openconfig.net/yang/interfaces"


@dataclass(frozen=True)
class NetconfPathProbeTarget:
    path: str
    model: str
    revision: str
    read_filter_xml: str
    test_config_xml: str


_OPENCONFIG_NETCONF_PROBE_TEMPLATES = (
    {
        "path": "/network-instances/network-instance/protocols/protocol/bgp",
        "model": "openconfig-bgp",
        "required_modules": {
            "openconfig-network-instance",
            "openconfig-bgp",
        },
        "filter_xml": (
            f'<network-instances xmlns="{OPENCONFIG_NETWORK_INSTANCE_NS}">'
            "<network-instance>"
            "<protocols>"
            "<protocol>"
            f'<bgp xmlns="{OPENCONFIG_BGP_NS}"/>'
            "</protocol>"
            "</protocols>"
            "</network-instance>"
            "</network-instances>"
        ),
    },
    {
        "path": "/network-instances/network-instance/policy-forwarding",
        "model": "openconfig-policy-forwarding",
        "required_modules": {
            "openconfig-network-instance",
            "openconfig-policy-forwarding",
        },
        "filter_xml": (
            f'<network-instances xmlns="{OPENCONFIG_NETWORK_INSTANCE_NS}">'
            "<network-instance>"
            f'<policy-forwarding xmlns="{OPENCONFIG_POLICY_FORWARDING_NS}"/>'
            "</network-instance>"
            "</network-instances>"
        ),
    },
    {
        "path": "/routing-policy",
        "model": "openconfig-routing-policy",
        "required_modules": {"openconfig-routing-policy"},
        "filter_xml": f'<routing-policy xmlns="{OPENCONFIG_ROUTING_POLICY_NS}"/>',
    },
    {
        "path": "/interfaces",
        "model": "openconfig-interfaces",
        "required_modules": {"openconfig-interfaces"},
        "filter_xml": f'<interfaces xmlns="{OPENCONFIG_INTERFACES_NS}"/>',
    },
)


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


def openconfig_netconf_probe_targets(
    supported_modules: dict[str, str],
) -> list[NetconfPathProbeTarget]:
    targets: list[NetconfPathProbeTarget] = []
    module_names = set(supported_modules.keys())
    for template in sorted(
        _OPENCONFIG_NETCONF_PROBE_TEMPLATES,
        key=lambda item: item["path"],
    ):
        if not template["required_modules"].issubset(module_names):
            continue
        model = str(template["model"])
        read_filter_xml = str(template["filter_xml"])
        targets.append(
            NetconfPathProbeTarget(
                path=str(template["path"]),
                model=model,
                revision=supported_modules.get(model, ""),
                read_filter_xml=read_filter_xml,
                test_config_xml=f"<config>{read_filter_xml}</config>",
            )
        )
    return targets


def classify_model_profile(
    *,
    vendor: str,
    model: str,
    os_version: str,
    supports_candidate: bool,
    supports_validate: bool,
    supported_modules: dict[str, str],
    verified_paths: dict[str, dict],
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
    verified_paths: dict[str, dict],
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
    verified_paths: dict[str, dict],
) -> list[dict]:
    paths: list[dict] = []
    for path, result in sorted(verified_paths.items()):
        paths.append(
            {
                "protocol": "openconfig_netconf",
                "model": result.get("model")
                or _best_matching_model(path, supported_modules),
                "revision": result.get("revision")
                or supported_modules.get(
                    result.get("model") or _best_matching_model(path, supported_modules), ""
                ),
                "path": path,
                "readable": result.get("readable", False),
                "writable": result.get("writable", False),
                "verified_on_device": True,
                "deviations": result.get("deviations", []),
                "notes": result.get("notes", []),
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
