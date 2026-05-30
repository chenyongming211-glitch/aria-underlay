import json

from aria_underlay_adapter.acceptance import offline_h3c


def test_offline_h3c_acceptance_reports_supported_surface(tmp_path, capsys):
    json_report = tmp_path / "offline-h3c-acceptance.json"
    summary = tmp_path / "offline-h3c-acceptance.txt"

    result = offline_h3c.main(
        [
            "--pretty",
            "--json-report",
            str(json_report),
            "--summary",
            str(summary),
        ]
    )

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 0
    assert report["runner"] == "offline-h3c-acceptance"
    assert report["vendor"] == "h3c"
    assert report["profile_name"] == "comware7-vlan-real"
    assert report["status"] == "passed"
    assert report["scenario_count"] == 5
    assert report["passed"] == 5
    assert report["failed"] == 0
    assert {scenario["name"] for scenario in report["scenarios"]} == {
        "vlan_access_description",
        "trunk_allowed_vlans",
        "ipv4_acl_rules",
        "acl_interface_binding",
        "explicit_delete_cleanup",
    }
    assert all(scenario["status"] == "passed" for scenario in report["scenarios"])

    covered_surface = {
        item
        for scenario in report["scenarios"]
        for item in scenario["surface"]
    }
    assert covered_surface == {
        "vlan_create",
        "vlan_description",
        "access_interface",
        "interface_description",
        "trunk_interface",
        "ipv4_advanced_acl",
        "acl_rule_description",
        "acl_interface_binding",
        "delete_vlan",
        "delete_acl",
        "delete_acl_binding",
    }

    assert json.loads(json_report.read_text()) == report
    assert "Offline H3C acceptance: passed (5/5)" in captured.err
    assert summary.read_text() == captured.err


def test_offline_h3c_acceptance_includes_rendered_xml_and_state_counts():
    report = offline_h3c.run_acceptance()

    assert report["status"] == "passed"
    for scenario in report["scenarios"]:
        assert scenario["xml_bytes"] > 0
        assert scenario["changed"] is True
        assert scenario["observed_counts"].keys() == {
            "vlans",
            "interfaces",
            "acls",
            "acl_bindings",
        }
