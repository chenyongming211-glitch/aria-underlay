from pathlib import Path

import pytest

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.state_parsers.h3c import H3cStateParser
from aria_underlay_adapter.state_parsers.huawei import HuaweiStateParser


FIXTURES = Path(__file__).parent / "fixtures" / "state_parsers"


def test_huawei_parser_reads_fixture_vlan_and_interfaces():
    state = HuaweiStateParser().parse_running(
        (FIXTURES / "huawei" / "vrp8_running.xml").read_text()
    )

    assert state["vlans"] == [
        {"vlan_id": 100, "name": "prod", "description": "production vlan"},
        {"vlan_id": 200, "name": "backup", "description": None},
    ]
    assert state["interfaces"] == [
        {
            "name": "GE1/0/1",
            "admin_state": "up",
            "description": "server uplink",
            "mode": {
                "kind": "access",
                "access_vlan": 100,
                "native_vlan": None,
                "allowed_vlans": [],
            },
        },
        {
            "name": "GE1/0/2",
            "admin_state": "up",
            "description": "core trunk",
            "mode": {
                "kind": "trunk",
                "access_vlan": None,
                "native_vlan": 100,
                "allowed_vlans": [100, 200],
            },
        },
    ]


def test_h3c_parser_reads_fixture_vlan_and_interfaces():
    state = H3cStateParser().parse_running(
        (FIXTURES / "h3c" / "comware7_running.xml").read_text()
    )

    assert state["vlans"] == [
        {"vlan_id": 100, "name": "prod", "description": "production vlan"},
        {"vlan_id": 200, "name": "backup", "description": None},
    ]
    assert state["interfaces"][0]["name"] == "GigabitEthernet1/0/1"
    assert state["interfaces"][0]["mode"] == {
        "kind": "access",
        "access_vlan": 100,
        "native_vlan": None,
        "allowed_vlans": [],
    }
    assert state["interfaces"][1]["mode"] == {
        "kind": "trunk",
        "access_vlan": None,
        "native_vlan": 100,
        "allowed_vlans": [100, 200],
    }


@pytest.mark.parametrize(
    ("parser", "fixture", "interface_name", "mode"),
    [
        (
            HuaweiStateParser(),
            FIXTURES / "huawei" / "vrp8_namespaced.xml",
            "GE1/0/3",
            {
                "kind": "access",
                "access_vlan": 300,
                "native_vlan": None,
                "allowed_vlans": [],
            },
        ),
        (
            H3cStateParser(),
            FIXTURES / "h3c" / "comware7_namespaced.xml",
            "GigabitEthernet1/0/3",
            {
                "kind": "trunk",
                "access_vlan": None,
                "native_vlan": 300,
                "allowed_vlans": [300, 301],
            },
        ),
    ],
)
def test_fixture_parsers_read_namespaced_xml(parser, fixture, interface_name, mode):
    state = parser.parse_running(fixture.read_text())

    assert state["vlans"] == [
        {
            "vlan_id": 300,
            "name": "ns-prod",
            "description": "namespaced production vlan",
        }
    ]
    assert state["interfaces"][0]["name"] == interface_name
    assert state["interfaces"][0]["mode"] == mode


def test_huawei_parser_normalizes_port_mode_kind_case():
    state = HuaweiStateParser().parse_running(
        """
        <data>
          <interfaces>
            <interface>
              <name>GE1/0/1</name>
              <port-mode>
                <kind>ACCESS</kind>
                <access-vlan>100</access-vlan>
              </port-mode>
            </interface>
            <interface>
              <name>GE1/0/2</name>
              <port-mode>
                <kind>Trunk</kind>
                <native-vlan>100</native-vlan>
                <allowed-vlans>
                  <vlan-id>100</vlan-id>
                  <vlan-id>200</vlan-id>
                </allowed-vlans>
              </port-mode>
            </interface>
          </interfaces>
        </data>
        """
    )

    assert state["interfaces"][0]["mode"]["kind"] == "access"
    assert state["interfaces"][1]["mode"]["kind"] == "trunk"
    assert state["interfaces"][1]["mode"]["allowed_vlans"] == [100, 200]


@pytest.mark.parametrize(
    ("xml", "summary"),
    [
        (
            "<data><vlans><vlan><name>prod</name></vlan></vlans></data>",
            "missing required text: vlan/vlan-id",
        ),
        (
            "<data><vlans><vlan><vlan-id>4095</vlan-id></vlan></vlans></data>",
            "invalid VLAN ID 4095",
        ),
        (
            """
            <data>
              <interfaces>
                <interface><name>GE1/0/1</name><port-mode><kind>access</kind><access-vlan>100</access-vlan></port-mode></interface>
                <interface><name>GE1/0/1</name><port-mode><kind>access</kind><access-vlan>100</access-vlan></port-mode></interface>
              </interfaces>
            </data>
            """,
            "duplicate interface GE1/0/1",
        ),
        (
            """
            <data>
              <interfaces>
                <interface><name>GE1/0/1</name><port-mode><kind>hybrid</kind></port-mode></interface>
              </interfaces>
            </data>
            """,
            "unknown port mode hybrid",
        ),
    ],
)
def test_huawei_parser_fails_closed_for_invalid_xml(xml, summary):
    with pytest.raises(AdapterError) as exc:
        HuaweiStateParser().parse_running(xml)

    assert exc.value.code == "NETCONF_STATE_PARSE_FAILED"
    assert summary in exc.value.raw_error_summary


@pytest.mark.parametrize(
    ("parser", "fixture", "summary"),
    [
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "missing_vlan_id.xml",
            "missing required text: vlan/vlan-id",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "duplicate_vlan.xml",
            "duplicate VLAN 100",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "invalid_vlan.xml",
            "invalid VLAN ID 4095",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "duplicate_interface.xml",
            "duplicate interface GE1/0/1",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "empty_interface_name.xml",
            "missing required text: interface/name",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "unknown_mode.xml",
            "unknown port mode hybrid",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "trunk_without_vlans.xml",
            "trunk port mode has no native or allowed VLAN",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "duplicate_allowed_vlan.xml",
            "trunk port mode has duplicate allowed VLAN",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "missing_vlan_id.xml",
            "missing required text: vlan/vlan-id",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "duplicate_vlan.xml",
            "duplicate VLAN 100",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "invalid_vlan.xml",
            "invalid VLAN ID 0",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "duplicate_interface.xml",
            "duplicate interface GigabitEthernet1/0/1",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "empty_interface_name.xml",
            "missing required text: interface/name",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "unknown_mode.xml",
            "unknown port mode hybrid",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "trunk_without_vlans.xml",
            "trunk port mode has no native or allowed VLAN",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "duplicate_allowed_vlan.xml",
            "trunk port mode has duplicate allowed VLAN",
        ),
    ],
)
def test_fixture_parsers_fail_closed_for_negative_fixtures(parser, fixture, summary):
    with pytest.raises(AdapterError) as exc:
        parser.parse_running(fixture.read_text())

    assert exc.value.code == "NETCONF_STATE_PARSE_FAILED"
    assert summary in exc.value.raw_error_summary
