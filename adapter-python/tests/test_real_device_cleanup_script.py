from __future__ import annotations

import importlib.util
from pathlib import Path
from xml.etree import ElementTree

import pytest


SCRIPT_PATH = Path(__file__).resolve().parents[2] / "scripts" / "real_device_cleanup.py"


def _load_cleanup_module():
    spec = importlib.util.spec_from_file_location("real_device_cleanup", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_access_cleanup_payload_restores_pvid_by_ifindex():
    cleanup = _load_cleanup_module()

    payload = cleanup.build_access_cleanup_payload("GigabitEthernet1/0/18", 1)
    root = ElementTree.fromstring(payload)

    assert root.tag.endswith("config")
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}AccessInterfaces") is not None
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}IfIndex").text == "18"
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}PVID").text == "1"


def test_trunk_cleanup_payload_restores_allowed_vlans():
    cleanup = _load_cleanup_module()

    payload = cleanup.build_trunk_cleanup_payload("Ten-GigabitEthernet1/0/44", [1159, 1259])
    root = ElementTree.fromstring(payload)

    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}TrunkInterfaces") is not None
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}IfIndex").text == "44"
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}PermitVlanList").text == "1159,1259"


def test_vlan_delete_payload_uses_netconf_delete_operation():
    cleanup = _load_cleanup_module()

    payload = cleanup.build_vlan_delete_payload(4093)
    root = ElementTree.fromstring(payload)
    vlan = root.find(".//{http://www.h3c.com/netconf/config:1.0}VLANID")

    assert vlan is not None
    assert vlan.attrib["{urn:ietf:params:xml:ns:netconf:base:1.0}operation"] == "delete"
    assert vlan.find("{http://www.h3c.com/netconf/config:1.0}ID").text == "4093"


def test_interface_description_cleanup_payload_restores_description():
    cleanup = _load_cleanup_module()

    payload = cleanup.build_description_cleanup_payload(
        "GigabitEthernet1/0/18",
        "server access",
        clear=False,
    )
    root = ElementTree.fromstring(payload)

    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}Ifmgr") is not None
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}IfIndex").text == "18"
    assert root.find(".//{http://www.h3c.com/netconf/config:1.0}Description").text == (
        "server access"
    )


def test_interface_description_cleanup_payload_can_clear_description():
    cleanup = _load_cleanup_module()

    payload = cleanup.build_description_cleanup_payload(
        "GigabitEthernet1/0/18",
        None,
        clear=True,
    )
    root = ElementTree.fromstring(payload)
    description = root.find(".//{http://www.h3c.com/netconf/config:1.0}Description")

    assert description is not None
    assert description.attrib["{urn:ietf:params:xml:ns:netconf:base:1.0}operation"] == "delete"


def test_execute_requires_yes_unless_dry_run():
    cleanup = _load_cleanup_module()
    args = cleanup.parse_args(
        [
            "--host",
            "10.0.0.1",
            "--secret-ref",
            "lab/h3c",
            "--delete-vlan",
            "4093",
        ]
    )

    with pytest.raises(SystemExit, match="refusing to connect without --yes"):
        cleanup.validate_safety_gate(args)

    dry_run_args = cleanup.parse_args(
        [
            "--host",
            "10.0.0.1",
            "--secret-ref",
            "lab/h3c",
            "--delete-vlan",
            "4093",
            "--dry-run",
        ]
    )
    cleanup.validate_safety_gate(dry_run_args)
