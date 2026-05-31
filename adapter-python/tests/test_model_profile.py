from aria_underlay_adapter.model_profile import (
    classify_model_profile,
    extract_yang_modules_from_capabilities,
)


def test_extracts_openconfig_modules_from_netconf_capabilities():
    modules = extract_yang_modules_from_capabilities(
        [
            "urn:ietf:params:netconf:capability:candidate:1.0",
            "urn:ietf:params:netconf:capability:validate:1.1",
            "http://openconfig.net/yang/network-instance?module=openconfig-network-instance&revision=2024-10-30",
            "http://openconfig.net/yang/bgp?module=openconfig-bgp&revision=2024-10-30",
            "http://openconfig.net/yang/routing-policy?module=openconfig-routing-policy&revision=2024-10-30",
        ]
    )

    assert modules["openconfig-network-instance"] == "2024-10-30"
    assert modules["openconfig-bgp"] == "2024-10-30"
    assert modules["openconfig-routing-policy"] == "2024-10-30"


def test_classifies_bgp_write_safe_only_with_required_paths_and_transaction_support():
    profile = classify_model_profile(
        vendor="h3c",
        model="lab-model",
        os_version="lab-os",
        supports_candidate=True,
        supports_validate=True,
        supported_modules={
            "openconfig-network-instance": "2024-10-30",
            "openconfig-bgp": "2024-10-30",
            "openconfig-routing-policy": "2024-10-30",
        },
        verified_paths={
            "/network-instances/network-instance/protocols/protocol/bgp": {
                "readable": True,
                "writable": True,
            },
            "/routing-policy": {
                "readable": True,
                "writable": True,
            },
        },
    )

    assert profile["bgp_write_readiness"] == "write_safe"
    assert profile["pbr_write_readiness"] == "write_rejected"


def test_classifies_module_only_support_as_rejected_for_writes():
    profile = classify_model_profile(
        vendor="h3c",
        model="lab-model",
        os_version="lab-os",
        supports_candidate=True,
        supports_validate=True,
        supported_modules={
            "openconfig-network-instance": "2024-10-30",
            "openconfig-bgp": "2024-10-30",
            "openconfig-routing-policy": "2024-10-30",
        },
        verified_paths={},
    )

    assert profile["bgp_write_readiness"] == "write_rejected"
    assert "missing verified path" in profile["rejection_reasons"][0]
