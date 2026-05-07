import json
from pathlib import Path

import pytest

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


def test_validator_help_documents_default_full_scope(capsys):
    with pytest.raises(SystemExit) as exc:
        validator.main(["--help"])

    captured = capsys.readouterr()
    help_text = " ".join(captured.out.split())

    assert exc.value.code == 0
    assert (
        "If no scope option is provided, the validator parses full observed state."
        in help_text
    )


def test_validator_manifest_outputs_batch_summary_for_successful_samples(
    tmp_path, capsys
):
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "samples": [
                    {
                        "name": "huawei-vrp8",
                        "vendor": "huawei",
                        "xml": str(FIXTURES / "huawei" / "vrp8_running.xml"),
                        "scope": {
                            "vlans": [100],
                            "interfaces": ["GE1/0/1"],
                        },
                    },
                    {
                        "name": "h3c-comware7",
                        "vendor": "h3c",
                        "xml": str(FIXTURES / "h3c" / "comware7_running.xml"),
                    },
                ]
            }
        )
    )

    result = validator.main(["--manifest", str(manifest)])

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert report["ok"] is True
    assert report["sample_count"] == 2
    assert report["passed"] == 2
    assert report["failed"] == 0
    assert report["samples"][0]["name"] == "huawei-vrp8"
    assert report["samples"][0]["ok"] is True
    assert report["samples"][0]["summary"]["vendor"] == "huawei"
    assert report["samples"][0]["summary"]["vlan_count"] == 1
    assert report["samples"][0]["summary"]["interface_count"] == 1
    assert report["samples"][1]["summary"]["profile_name"] == "comware7-state-real"


def test_validator_manifest_resolves_xml_paths_relative_to_manifest(tmp_path, capsys):
    sample_dir = tmp_path / "samples"
    sample_dir.mkdir()
    xml = sample_dir / "huawei.xml"
    xml.write_text((FIXTURES / "huawei" / "vrp8_running.xml").read_text())
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "samples": [
                    {
                        "name": "relative-huawei",
                        "vendor": "huawei",
                        "xml": "samples/huawei.xml",
                    }
                ]
            }
        )
    )

    result = validator.main(["--manifest", str(manifest)])

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 0
    assert captured.err == ""
    assert report["samples"][0]["summary"]["vlan_count"] == 2


def test_validator_manifest_reports_all_samples_when_one_parse_fails(
    tmp_path, capsys
):
    invalid_xml = tmp_path / "invalid.xml"
    invalid_xml.write_text("<data><vlans><vlan><name>prod</name></vlan></vlans></data>")
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "samples": [
                    {
                        "name": "valid-huawei",
                        "vendor": "huawei",
                        "xml": str(FIXTURES / "huawei" / "vrp8_running.xml"),
                    },
                    {
                        "name": "invalid-huawei",
                        "vendor": "huawei",
                        "xml": "invalid.xml",
                    },
                ]
            }
        )
    )

    result = validator.main(["--manifest", str(manifest)])

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 1
    assert captured.err == ""
    assert report["ok"] is False
    assert report["sample_count"] == 2
    assert report["passed"] == 1
    assert report["failed"] == 1
    assert report["samples"][0]["ok"] is True
    assert report["samples"][1]["ok"] is False
    assert report["samples"][1]["error"]["code"] == "NETCONF_STATE_PARSE_FAILED"


def test_validator_manifest_returns_structured_error_for_invalid_shape(
    tmp_path, capsys
):
    manifest = tmp_path / "samples.json"
    manifest.write_text(json.dumps({"samples": {}}))

    result = validator.main(["--manifest", str(manifest)])

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "STATE_PARSER_MANIFEST_INVALID"
    assert "samples must be a list" in error["raw_error_summary"]


def test_validator_manifest_returns_structured_error_for_invalid_scope_values(
    tmp_path, capsys
):
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "samples": [
                    {
                        "name": "bad-scope",
                        "vendor": "huawei",
                        "xml": str(FIXTURES / "huawei" / "vrp8_running.xml"),
                        "scope": {
                            "vlans": ["100"],
                            "interfaces": ["GE1/0/1"],
                        },
                    }
                ]
            }
        )
    )

    result = validator.main(["--manifest", str(manifest)])

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "STATE_PARSER_MANIFEST_INVALID"
    assert "scope.vlans[0] must be an integer" in error["raw_error_summary"]


def test_validator_manifest_rejects_single_sample_arguments(tmp_path, capsys):
    manifest = tmp_path / "samples.json"
    manifest.write_text(json.dumps({"samples": []}))

    result = validator.main(["--manifest", str(manifest), "--vendor", "huawei"])

    captured = capsys.readouterr()
    error = json.loads(captured.err)

    assert result == 1
    assert captured.out == ""
    assert error["code"] == "STATE_PARSER_ARGUMENT_INVALID"
    assert "--manifest cannot be combined with --vendor" in error["raw_error_summary"]


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
