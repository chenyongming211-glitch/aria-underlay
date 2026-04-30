import json

from aria_underlay_adapter.renderers import snapshot


def _write_desired_state(path, *, vlan_id=100):
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
                        "mode": {
                            "kind": "access",
                            "access_vlan": 100,
                        },
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
    assert "<config" in report["xml"]
    assert "<ns0:id>100</ns0:id>" in report["xml"]
    assert "<ns1:name>GE1/0/1</ns1:name>" in report["xml"]


def test_render_snapshot_pretty_prints_json(tmp_path, capsys):
    desired_state = tmp_path / "desired.json"
    _write_desired_state(desired_state)

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
    assert report["profile_name"] == "comware7-skeleton"


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
