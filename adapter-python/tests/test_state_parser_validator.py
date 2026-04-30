import json
from pathlib import Path

from aria_underlay_adapter.state_parsers import validator


FIXTURES = Path(__file__).parent / "fixtures" / "state_parsers"


def test_validator_outputs_observed_state_json_for_huawei_fixture(capsys):
    result = validator.main(
        [
            "--vendor",
            "huawei",
            "--xml",
            str(FIXTURES / "huawei" / "vrp8_running.xml"),
        ]
    )

    captured = capsys.readouterr()
    state = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert [vlan["vlan_id"] for vlan in state["vlans"]] == [100, 200]
    assert [interface["name"] for interface in state["interfaces"]] == [
        "GE1/0/1",
        "GE1/0/2",
    ]


def test_validator_filters_observed_state_by_scope(capsys):
    result = validator.main(
        [
            "--vendor",
            "huawei",
            "--xml",
            str(FIXTURES / "huawei" / "vrp8_running.xml"),
            "--vlan",
            "100",
            "--interface",
            "GE1/0/1",
        ]
    )

    captured = capsys.readouterr()
    state = json.loads(captured.out)

    assert result == 0
    assert [vlan["vlan_id"] for vlan in state["vlans"]] == [100]
    assert [interface["name"] for interface in state["interfaces"]] == ["GE1/0/1"]


def test_validator_returns_structured_error_for_unsupported_vendor(capsys):
    result = validator.main(
        [
            "--vendor",
            "unknown",
            "--xml",
            str(FIXTURES / "huawei" / "vrp8_running.xml"),
        ]
    )

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "STATE_PARSER_VENDOR_UNSUPPORTED"
    assert "vendor=unknown" in error["raw_error_summary"]


def test_validator_returns_structured_error_for_invalid_xml(tmp_path, capsys):
    xml = tmp_path / "invalid.xml"
    xml.write_text("<data><vlans><vlan><name>prod</name></vlan></vlans></data>")

    result = validator.main(["--vendor", "huawei", "--xml", str(xml)])

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "NETCONF_STATE_PARSE_FAILED"
    assert "missing required text: vlan/vlan-id" in error["raw_error_summary"]
