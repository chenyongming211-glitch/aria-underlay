import importlib.util


def test_legacy_placeholder_modules_are_not_shipped():
    modules = [
        "aria_underlay_adapter.diff",
        "aria_underlay_adapter.rollback",
        "aria_underlay_adapter.state",
        "aria_underlay_adapter.backends.napalm_backend",
        "aria_underlay_adapter.backends.netmiko_backend",
    ]

    for module in modules:
        assert importlib.util.find_spec(module) is None, module
