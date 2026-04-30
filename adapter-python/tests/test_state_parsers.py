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
