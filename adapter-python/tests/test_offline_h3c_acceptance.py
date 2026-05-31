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
        assert scenario["readback_xml_bytes"] > 0
        assert scenario["changed"] is True
        assert scenario["parser_profile_name"] == "comware7-state-real"
        assert scenario["stages"] == ["render", "apply", "parse", "verify"]
        assert scenario["observed_counts"].keys() == {
            "vlans",
            "interfaces",
            "acls",
            "acl_bindings",
        }
        assert scenario["parsed_counts"] == scenario["observed_counts"]


def test_offline_h3c_acceptance_reports_change_plan_metadata():
    report = offline_h3c.run_acceptance()

    for scenario in report["scenarios"]:
        assert "change_plan" in scenario
        assert scenario["change_plan"]["stages"]
        assert scenario["change_plan"]["blast_radius"] in {
            "local_interface_or_vlan",
            "policy_reference",
        }
        assert "dependency_edges" in scenario["change_plan"]
        assert "rollback_order" in scenario["change_plan"]

    by_name = {scenario["name"]: scenario for scenario in report["scenarios"]}
    assert (
        by_name["vlan_access_description"]["change_plan"]["blast_radius"]
        == "local_interface_or_vlan"
    )
    assert (
        by_name["explicit_delete_cleanup"]["change_plan"]["blast_radius"]
        == "policy_reference"
    )


def test_offline_h3c_acceptance_reports_pbr_bgp_read_only_audit():
    report = offline_h3c.run_acceptance()

    assert report["read_only_audits"] == [
        {
            "name": "pbr_bgp_high_risk_read_only",
            "status": "passed",
            "surface": ["pbr", "bgp"],
            "stages": ["parse", "audit"],
            "changed": False,
            "write_decision": "read_only",
            "features_present": ["bgp", "pbr"],
            "blast_radius": "routing_control_plane",
            "unsupported_paths": [
                "bgp: no path-level write evidence",
                "pbr: no path-level write evidence",
            ],
            "touched_scope": {
                "affected_vrfs": ["tenant-a"],
                "bgp_as_numbers": [65001],
                "bgp_neighbors": ["192.0.2.1"],
                "route_policy_refs": ["rp-in"],
                "pbr_policy_refs": ["pbr-tenant-a"],
                "acl_refs": [3999],
                "interfaces": ["GigabitEthernet1/0/13"],
                "raw_paths": ["/data/top/BGP", "/data/top/PBR"],
            },
            "pbr": {
                "present": True,
                "blast_radius": "policy_reference",
                "policies": ["pbr-tenant-a"],
                "acl_references": [3999],
                "interfaces": ["GigabitEthernet1/0/13"],
                "raw_paths": ["/data/top/PBR"],
            },
            "bgp": {
                "present": True,
                "blast_radius": "routing_control_plane",
                "as_numbers": [65001],
                "vrfs": ["tenant-a"],
                "neighbors": ["192.0.2.1"],
                "policy_references": ["rp-in"],
                "raw_paths": ["/data/top/BGP"],
            },
            "warnings": [
                "BGP config detected; read-only audit only until path-level write evidence exists",
                "PBR config detected; read-only audit only until path-level write evidence exists",
            ],
        }
    ]


def test_offline_h3c_acceptance_summary_marks_parser_loop(capsys):
    result = offline_h3c.main([])

    captured = capsys.readouterr()

    assert result == 0
    assert "parser_loop=true" in captured.err
    assert "vrfs=tenant-a" in captured.err
    assert "bgp_neighbors=192.0.2.1" in captured.err
    assert "route_policies=rp-in" in captured.err
    assert "pbr_policies=pbr-tenant-a" in captured.err
    assert "acl_refs=3999" in captured.err
    assert "interfaces=GigabitEthernet1/0/13" in captured.err
