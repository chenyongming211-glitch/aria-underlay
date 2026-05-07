import json
from xml.etree import ElementTree

from aria_underlay_adapter.renderers import snapshot
from aria_underlay_adapter.renderers.h3c import H3C_COMWARE_CONFIG_NAMESPACE
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer
from aria_underlay_adapter.renderers.xml import NETCONF_BASE_NAMESPACE


def _write_desired_state(path, *, vlan_id=100, mode=None):
    if mode is None:
        mode = {
            "kind": "access",
            "access_vlan": 100,
        }
    path.write_text(
        json.dumps(
            {
                "vlans": [
                    {
                        "vlan_id": vlan_id,
                        "name": "prod",
                        "description": "production vlan",
                    }
                ],
                "interfaces": [
                    {
                        "name": "GE1/0/1",
                        "admin_state": "up",
                        "description": "server uplink",
                        "mode": mode,
                    }
                ],
            }
        )
    )


def _write_h3c_desired_state(path, *, vlan_id=100, mode=None):
    if mode is None:
        mode = {
            "kind": "access",
            "access_vlan": 100,
        }
    path.write_text(
        json.dumps(
            {
                "vlans": [
                    {
                        "vlan_id": vlan_id,
                        "name": "prod",
                    }
                ],
                "interfaces": [
                    {
                        "name": "GigabitEthernet1/0/13",
                        "admin_state": "up",
                        "mode": mode,
                    }
                ],
            }
        )
    )


def test_render_snapshot_outputs_xml_report_for_huawei(tmp_path, capsys):
    desired_state = tmp_path / "desired.json"
    _write_desired_state(desired_state)

    result = snapshot.main(
        ["--vendor", "huawei", "--desired-state", str(desired_state)]
    )

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert report["vendor"] == "huawei"
    assert report["profile_name"] == "vrp8-skeleton"
    assert report["production_ready"] is False
    assert report["vlan_count"] == 1
    assert report["interface_count"] == 1

    root = ElementTree.fromstring(report["xml"])
    renderer = HuaweiRenderer()
    assert root.tag == f"{{{NETCONF_BASE_NAMESPACE}}}config"
    assert root.find(f".//{{{renderer.VLAN_NAMESPACE}}}id").text == "100"
    assert root.find(f".//{{{renderer.IFACE_NAMESPACE}}}name").text == "GE1/0/1"


def test_render_snapshot_pretty_prints_json(tmp_path, capsys):
    desired_state = tmp_path / "desired.json"
    _write_h3c_desired_state(desired_state)

    result = snapshot.main(
        [
            "--vendor",
            "h3c",
            "--desired-state",
            str(desired_state),
            "--pretty",
        ]
    )

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert captured.out.startswith("{\n")
    assert report["vendor"] == "h3c"
    assert report["profile_name"] == "comware7-vlan-real"
    assert report["production_ready"] is True
    assert "GigabitEthernet1/0/13" not in report["xml"]

    root = ElementTree.fromstring(report["xml"])
    assert root.tag == f"{{{NETCONF_BASE_NAMESPACE}}}config"
    assert root.find(f".//{{{H3C_COMWARE_CONFIG_NAMESPACE}}}VLAN") is not None
    assert root.find(f".//{{{H3C_COMWARE_CONFIG_NAMESPACE}}}IfIndex").text == "13"


def test_render_snapshot_returns_structured_error_for_renderer_validation(
    tmp_path, capsys
):
    desired_state = tmp_path / "desired.json"
    _write_desired_state(desired_state, vlan_id=4095)

    result = snapshot.main(
        ["--vendor", "huawei", "--desired-state", str(desired_state)]
    )

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "RENDER_SNAPSHOT_FAILED"
    assert "range 1..4094" in error["raw_error_summary"]


def test_render_snapshot_returns_structured_error_for_invalid_trunk_mode(
    tmp_path, capsys
):
    desired_state = tmp_path / "desired.json"
    _write_h3c_desired_state(
        desired_state,
        mode={
            "kind": "trunk",
            "native_vlan": None,
            "allowed_vlans": [100, 100],
        },
    )

    result = snapshot.main(
        ["--vendor", "h3c", "--desired-state", str(desired_state)]
    )

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "RENDER_SNAPSHOT_FAILED"
    assert "duplicate allowed_vlans" in error["raw_error_summary"]


def test_render_snapshot_returns_structured_error_for_invalid_admin_state(
    tmp_path, capsys
):
    desired_state = tmp_path / "desired.json"
    _write_desired_state(desired_state)
    data = json.loads(desired_state.read_text())
    data["interfaces"][0]["admin_state"] = "disabled"
    desired_state.write_text(json.dumps(data))

    result = snapshot.main(
        ["--vendor", "huawei", "--desired-state", str(desired_state)]
    )

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "RENDER_SNAPSHOT_FAILED"
    assert "unknown admin state" in error["raw_error_summary"]


def test_render_snapshot_returns_structured_error_for_invalid_input_shape(
    tmp_path, capsys
):
    desired_state = tmp_path / "desired.json"
    desired_state.write_text(json.dumps([]))

    result = snapshot.main(
        ["--vendor", "huawei", "--desired-state", str(desired_state)]
    )

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "RENDER_SNAPSHOT_INPUT_INVALID"
    assert "desired state must be a JSON object" in error["raw_error_summary"]


def test_render_snapshot_returns_structured_error_for_unsupported_vendor(
    tmp_path, capsys
):
    desired_state = tmp_path / "desired.json"
    _write_desired_state(desired_state)

    result = snapshot.main(
        ["--vendor", "unknown", "--desired-state", str(desired_state)]
    )

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "RENDERER_VENDOR_UNSUPPORTED"
    assert "vendor=unknown" in error["raw_error_summary"]
