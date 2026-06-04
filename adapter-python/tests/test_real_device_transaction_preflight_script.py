from __future__ import annotations

import importlib.util
import json
from pathlib import Path


SCRIPT_PATH = (
    Path(__file__).resolve().parents[2] / "scripts" / "real_device_transaction_preflight.py"
)


def _load_preflight_module():
    spec = importlib.util.spec_from_file_location(
        "real_device_transaction_preflight",
        SCRIPT_PATH,
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_preflight_report_is_read_only_and_has_transaction_matrix():
    preflight = _load_preflight_module()

    report = preflight.build_report(
        host="192.0.2.10",
        ssh_port=22,
        netconf_port=830,
        vendor="h3c",
        model="S5560-54C-EI",
        os_version="Comware 7.1.070",
        ssh_reachable=True,
        netconf_reachable=True,
    )

    assert report["runner"] == "real-device-transaction-preflight"
    assert report["status"] == "ready_for_scoped_write_acceptance"
    assert report["read_only"] is True
    assert report["device"] == {
        "host": "192.0.2.10",
        "ssh_port": 22,
        "netconf_port": 830,
        "vendor": "h3c",
        "model": "S5560-54C-EI",
        "os_version": "Comware 7.1.070",
    }
    assert report["connectivity"] == {
        "ssh": {"port": 22, "reachable": True},
        "netconf": {"port": 830, "reachable": True},
    }
    assert [scenario["name"] for scenario in report["transaction_scenarios"]] == [
        "capability_and_strategy_probe",
        "dry_run_change_plan",
        "idempotent_apply_reuse",
        "apply_verify_report",
        "commit_failure_discards_candidate",
        "rollback_failure_enters_in_doubt",
        "startup_recovery_after_crash",
        "force_resolve_break_glass",
    ]
    assert all(
        scenario["requires_real_device"]
        for scenario in report["transaction_scenarios"]
    )
    assert any(
        scenario["requires_write"]
        for scenario in report["transaction_scenarios"]
    )


def test_preflight_status_blocks_when_netconf_is_unreachable():
    preflight = _load_preflight_module()

    report = preflight.build_report(
        host="192.0.2.10",
        ssh_port=22,
        netconf_port=830,
        vendor="h3c",
        model=None,
        os_version=None,
        ssh_reachable=True,
        netconf_reachable=False,
    )

    assert report["status"] == "blocked"
    assert report["blocking_reasons"] == ["netconf port is not reachable"]


def test_preflight_cli_writes_json_report(tmp_path, capsys):
    preflight = _load_preflight_module()
    report_path = tmp_path / "transaction-preflight.json"

    result = preflight.main(
        [
            "--host",
            "192.0.2.10",
            "--skip-connectivity-check",
            "--ssh-reachable",
            "--netconf-reachable",
            "--model",
            "S5560-54C-EI",
            "--json-report",
            str(report_path),
        ]
    )

    captured = capsys.readouterr()
    report = json.loads(captured.out)

    assert result == 0
    assert report["status"] == "ready_for_scoped_write_acceptance"
    assert report["model_ready_notes"] == [
        "Use the normal real-device acceptance probe only after selecting an absent VLAN/ACL and approved idle interfaces.",
        "Do not store passwords, private keys, or raw customer configuration in the report.",
    ]
    assert json.loads(report_path.read_text()) == report


def test_preflight_parser_has_no_password_argument():
    preflight = _load_preflight_module()
    help_text = preflight.parser().format_help()

    assert "--password" not in help_text
    assert "--private-key" not in help_text
