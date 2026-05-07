from pathlib import Path
from types import SimpleNamespace

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


def test_h3c_parser_reads_real_comware_vlan_shape_with_model_hint():
    state = H3cStateParser(model_hint="S5560-54C-EI").parse_running(
        """
        <data xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
          <top xmlns="http://www.h3c.com/netconf/config:1.0">
            <Ifmgr>
              <Interfaces>
                <Interface>
                  <IfIndex>13</IfIndex>
                  <Description>server access</Description>
                </Interface>
                <Interface>
                  <IfIndex>14</IfIndex>
                  <Description>core trunk</Description>
                </Interface>
              </Interfaces>
            </Ifmgr>
            <VLAN>
              <AccessInterfaces>
                <Interface>
                  <IfIndex>13</IfIndex>
                  <PVID>144</PVID>
                </Interface>
              </AccessInterfaces>
              <TrunkInterfaces>
                <Interface>
                  <IfIndex>14</IfIndex>
                  <PermitVlanList>30,50,1150-1153</PermitVlanList>
                </Interface>
              </TrunkInterfaces>
              <VLANs>
                <VLANID>
                  <ID>144</ID>
                  <Name>tenant-access</Name>
                </VLANID>
                <VLANID>
                  <ID>1150</ID>
                </VLANID>
              </VLANs>
            </VLAN>
          </top>
        </data>
        """
    )

    assert state["vlans"] == [
        {"vlan_id": 144, "name": "tenant-access", "description": None},
        {"vlan_id": 1150, "name": None, "description": None},
    ]
    assert state["interfaces"] == [
        {
            "name": "GigabitEthernet1/0/13",
            "admin_state": None,
            "description": "server access",
            "mode": {
                "kind": "access",
                "access_vlan": 144,
                "native_vlan": None,
                "allowed_vlans": [],
            },
        },
        {
            "name": "GigabitEthernet1/0/14",
            "admin_state": None,
            "description": "core trunk",
            "mode": {
                "kind": "trunk",
                "access_vlan": None,
                "native_vlan": None,
                "allowed_vlans": [30, 50, 1150, 1151, 1152, 1153],
            },
        },
    ]


def test_h3c_scoped_parser_skips_unrequested_ifindex_without_model_hint():
    scope = SimpleNamespace(
        full=False,
        vlan_ids=[4093],
        interface_names=["GigabitEthernet1/0/1"],
    )

    state = H3cStateParser().parse_running(
        """
        <data xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
          <top xmlns="http://www.h3c.com/netconf/config:1.0">
            <VLAN>
              <AccessInterfaces>
                <Interface>
                  <IfIndex>1</IfIndex>
                  <PVID>1</PVID>
                </Interface>
                <Interface>
                  <IfIndex>13</IfIndex>
                  <PVID>144</PVID>
                </Interface>
              </AccessInterfaces>
              <VLANs>
                <VLANID><ID>144</ID></VLANID>
                <VLANID><ID>4093</ID></VLANID>
              </VLANs>
            </VLAN>
          </top>
        </data>
        """,
        scope=scope,
    )

    assert state["vlans"] == [
        {"vlan_id": 4093, "name": None, "description": None},
    ]
    assert [interface["name"] for interface in state["interfaces"]] == [
        "GigabitEthernet1/0/1",
    ]


def test_h3c_parser_maps_s6800_physical_ifindex_ranges():
    state = H3cStateParser(model_hint="S6800-54QF").parse_running(
        """
        <data xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
          <top xmlns="http://www.h3c.com/netconf/config:1.0">
            <VLAN>
              <AccessInterfaces>
                <Interface>
                  <IfIndex>47</IfIndex>
                  <PVID>6</PVID>
                </Interface>
              </AccessInterfaces>
              <TrunkInterfaces>
                <Interface>
                  <IfIndex>49</IfIndex>
                  <PermitVlanList>1,1003-1004</PermitVlanList>
                </Interface>
              </TrunkInterfaces>
              <VLANs>
                <VLANID><ID>6</ID></VLANID>
                <VLANID><ID>1003</ID></VLANID>
                <VLANID><ID>1004</ID></VLANID>
              </VLANs>
            </VLAN>
          </top>
        </data>
        """
    )

    assert [interface["name"] for interface in state["interfaces"]] == [
        "Ten-GigabitEthernet1/0/47",
        "FortyGigE1/0/49",
    ]


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


def test_huawei_parser_rejects_non_integer_vlan_scope_with_context():
    scope = SimpleNamespace(full=False, vlan_ids=["not-a-vlan"], interface_names=[])

    with pytest.raises(AdapterError) as exc:
        HuaweiStateParser().parse_running(
            (FIXTURES / "huawei" / "vrp8_running.xml").read_text(),
            scope=scope,
        )

    assert exc.value.code == "NETCONF_STATE_PARSE_FAILED"
    assert "scope.vlan_ids[0] must be an integer" in exc.value.raw_error_summary


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
            FIXTURES / "negative" / "huawei" / "missing_port_mode.xml",
            "missing required element: interface GE1/0/1/port-mode",
        ),
        (
            HuaweiStateParser(),
            FIXTURES / "negative" / "huawei" / "non_integer_access_vlan.xml",
            "invalid VLAN ID abc",
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
            FIXTURES / "negative" / "h3c" / "missing_port_mode.xml",
            "missing required element: interface GigabitEthernet1/0/1/port-mode",
        ),
        (
            H3cStateParser(),
            FIXTURES / "negative" / "h3c" / "non_integer_access_vlan.xml",
            "invalid VLAN ID abc",
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
