import importlib.util
from pathlib import Path


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


def test_netconf_backend_helpers_are_split_from_main_backend():
    package_root = Path(__file__).resolve().parents[1] / "aria_underlay_adapter"
    backend_root = package_root / "backends"
    netconf_path = backend_root / "netconf.py"

    helper_modules = [
        "netconf_errors.py",
        "netconf_hostkey.py",
        "netconf_state.py",
    ]
    for helper in helper_modules:
        assert (backend_root / helper).exists(), helper

    assert len(netconf_path.read_text(encoding="utf-8").splitlines()) <= 800
