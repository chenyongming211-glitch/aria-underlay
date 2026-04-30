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


def test_validator_pretty_prints_observed_state_json(capsys):
    result = validator.main(
        [
            "--vendor",
            "huawei",
            "--xml",
            str(FIXTURES / "huawei" / "vrp8_running.xml"),
            "--pretty",
        ]
    )

    captured = capsys.readouterr()
    state = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert captured.out.startswith("{\n")
    assert '\n  "interfaces": [' in captured.out
    assert [vlan["vlan_id"] for vlan in state["vlans"]] == [100, 200]


def test_validator_outputs_summary_json(capsys):
    result = validator.main(
        [
            "--vendor",
            "huawei",
            "--xml",
            str(FIXTURES / "huawei" / "vrp8_running.xml"),
            "--summary",
        ]
    )

    captured = capsys.readouterr()
    summary = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert summary == {
        "fixture_verified": True,
        "interface_count": 2,
        "production_ready": False,
        "profile_name": "vrp8-state-fixture",
        "scope": {
            "full": True,
            "interface_names": [],
            "vlan_ids": [],
        },
        "vendor": "huawei",
        "vlan_count": 2,
    }


def test_validator_outputs_scoped_summary_json(capsys):
    result = validator.main(
        [
            "--vendor",
            "huawei",
            "--xml",
            str(FIXTURES / "huawei" / "vrp8_running.xml"),
            "--summary",
            "--vlan",
            "100",
            "--interface",
            "GE1/0/1",
        ]
    )

    captured = capsys.readouterr()
    summary = json.loads(captured.out)

    assert result == 0
    assert summary["vlan_count"] == 1
    assert summary["interface_count"] == 1
    assert summary["scope"] == {
        "full": False,
        "interface_names": ["GE1/0/1"],
        "vlan_ids": [100],
    }


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
