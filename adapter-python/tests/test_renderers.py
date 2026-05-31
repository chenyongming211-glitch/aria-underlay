from dataclasses import dataclass
from types import SimpleNamespace
from xml.etree import ElementTree

import pytest

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.common import _admin_state_text
from aria_underlay_adapter.renderers.common import RendererNamespaceProfile
from aria_underlay_adapter.renderers.h3c import H3cRenderer
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer
from aria_underlay_adapter.renderers.xml import NETCONF_BASE_NAMESPACE
from aria_underlay_adapter.renderers.xml import XmlElement, render_xml


@dataclass
class _Vlan:
    vlan_id: int
    name: str | None = None
    description: str | None = None


@dataclass
class _Interface:
    name: str
    admin_state: str | int
    description: str | None
    mode: dict | object


@dataclass
class _DesiredState:
    vlans: list
    interfaces: list
    acls: list | None = None
    acl_bindings: list | None = None
    delete_vlan_ids: list | None = None
    delete_acl_ids: list | None = None
    delete_acl_bindings: list | None = None

    def __post_init__(self):
        if self.acls is None:
            self.acls = []
        if self.acl_bindings is None:
            self.acl_bindings = []
        if self.delete_vlan_ids is None:
            self.delete_vlan_ids = []
        if self.delete_acl_ids is None:
            self.delete_acl_ids = []
        if self.delete_acl_bindings is None:
            self.delete_acl_bindings = []


@dataclass
class _AclEndpoint:
    address: str
    wildcard: str


@dataclass
class _AclRule:
    sequence: int
    action: str | int
    protocol: str | int
    source: _AclEndpoint | None = None
    destination: _AclEndpoint | None = None
    source_port_eq: int | None = None
    destination_port_eq: int | None = None
    description: str | None = None


@dataclass
class _Acl:
    acl_id: int
    kind: str | int | None = None
    name: str | None = None
    description: str | None = None
    rules: list[_AclRule] | None = None

    def __post_init__(self):
        if self.rules is None:
            self.rules = []


@dataclass
class _AclBinding:
    interface_name: str
    direction: str | int
    acl_id: int


def test_xml_renderer_escapes_text():
    xml = render_xml(XmlElement("description", children=["a & b < c"]))

    assert xml == "<description>a &amp; b &lt; c</description>"


def test_renderer_admin_state_text_matches_netconf_default_for_unspecified_values():
    assert _admin_state_text(0) == "up"
    assert _admin_state_text(None) == "up"
    assert _admin_state_text("DOWN") == "down"


def test_renderer_admin_state_text_rejects_unknown_values():
    with pytest.raises(ValueError, match="unknown admin state"):
        _admin_state_text("disabled")


@pytest.mark.parametrize("renderer", [HuaweiRenderer()])
def test_vendor_renderer_skeletons_are_not_production_ready(renderer):
    assert renderer.production_ready is False
    assert renderer.profile.production_ready is False
    assert renderer.profile.vendor == "huawei"
    assert renderer.profile.profile_name.endswith("-skeleton")
    assert renderer.VLAN_NAMESPACE.endswith(":skeleton")
    assert renderer.IFACE_NAMESPACE.endswith(":skeleton")


def test_h3c_renderer_is_production_ready():
    renderer = H3cRenderer()

    assert renderer.production_ready is True
    assert renderer.profile.production_ready is True
    assert renderer.profile.vendor == "h3c"
    assert renderer.profile.profile_name == "comware7-vlan-real"
    assert renderer.VLAN_NAMESPACE == "http://www.h3c.com/netconf/config:1.0"
    assert renderer.IFACE_NAMESPACE == "http://www.h3c.com/netconf/config:1.0"


@pytest.mark.parametrize(
    "profile_kwargs, message",
    [
        (
            {
                "vendor": "",
                "profile_name": "bad-skeleton",
                "vlan_namespace": "urn:aria:underlay:renderer:bad:vlan:skeleton",
                "interface_namespace": "urn:aria:underlay:renderer:bad:interface:skeleton",
            },
            "vendor is required",
        ),
        (
            {
                "vendor": "huawei",
                "profile_name": "bad profile",
                "vlan_namespace": "urn:aria:underlay:renderer:huawei:vlan:skeleton",
                "interface_namespace": "urn:aria:underlay:renderer:huawei:interface:skeleton",
            },
            "profile_name must be a stable token",
        ),
        (
            {
                "vendor": "huawei",
                "profile_name": "vrp8-skeleton",
                "vlan_namespace": "",
                "interface_namespace": "urn:aria:underlay:renderer:huawei:interface:skeleton",
            },
            "vlan_namespace is required",
        ),
        (
            {
                "vendor": "huawei",
                "profile_name": "vrp8-skeleton",
                "vlan_namespace": "urn:aria:underlay:renderer:huawei:shared:skeleton",
                "interface_namespace": "urn:aria:underlay:renderer:huawei:shared:skeleton",
            },
            "vlan_namespace and interface_namespace must be distinct",
        ),
        (
            {
                "vendor": "huawei",
                "profile_name": "vrp8-skeleton",
                "vlan_namespace": "urn:aria:underlay:renderer:huawei:vlan:skeleton",
                "interface_namespace": "urn:aria:underlay:renderer:huawei:interface:skeleton",
                "production_ready": True,
            },
            "production_ready profile cannot use skeleton markers",
        ),
    ],
)
def test_renderer_namespace_profile_fails_closed_for_invalid_fields(
    profile_kwargs, message
):
    with pytest.raises(ValueError, match=message):
        RendererNamespaceProfile(**profile_kwargs)


@pytest.mark.parametrize("renderer", [HuaweiRenderer()])
def test_vendor_renderer_builds_vlan_create_xml(renderer):
    xml = render_xml(
        renderer.render_vlan_create(
            _Vlan(vlan_id=100, name="prod", description="production vlan")
        )
    )

    assert "<ns0:id>100</ns0:id>" in xml
    assert "<ns0:name>prod</ns0:name>" in xml
    assert "<ns0:description>production vlan</ns0:description>" in xml


def test_h3c_renderer_builds_vlan_create_xml():
    xml = render_xml(H3cRenderer().render_vlan_create(_Vlan(vlan_id=100, name="prod")))
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().VLAN_NAMESPACE

    assert root.tag == f"{{{ns}}}VLANID"
    assert root.find(f"{{{ns}}}ID").text == "100"
    assert root.find(f"{{{ns}}}Name").text == "prod"


def test_h3c_renderer_builds_vlan_description_xml():
    xml = render_xml(
        H3cRenderer().render_vlan_create(
            _Vlan(vlan_id=100, name="prod", description="tenant vlan")
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().VLAN_NAMESPACE

    assert root.find(f"{{{ns}}}ID").text == "100"
    assert root.find(f"{{{ns}}}Name").text == "prod"
    assert root.find(f"{{{ns}}}Description").text == "tenant vlan"


@pytest.mark.parametrize("renderer", [HuaweiRenderer()])
def test_vendor_renderer_builds_vlan_delete_xml(renderer):
    xml = render_xml(renderer.render_vlan_delete(100))
    root = ElementTree.fromstring(xml)

    assert 'operation="delete"' in xml
    assert root.attrib[f"{{{NETCONF_BASE_NAMESPACE}}}operation"] == "delete"
    assert "<ns0:id>100</ns0:id>" in xml


def test_h3c_renderer_builds_vlan_delete_xml():
    xml = render_xml(H3cRenderer().render_vlan_delete(100))
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().VLAN_NAMESPACE

    assert root.tag == f"{{{ns}}}VLANID"
    assert root.attrib[f"{{{NETCONF_BASE_NAMESPACE}}}operation"] == "delete"
    assert root.find(f"{{{ns}}}ID").text == "100"


@pytest.mark.parametrize("renderer", [HuaweiRenderer()])
def test_vendor_renderer_builds_access_interface_xml(renderer):
    xml = render_xml(
        renderer.render_interface_update(
            _Interface(
                name="GE1/0/1",
                admin_state="up",
                description="server uplink",
                mode={"kind": "access", "access_vlan": 100},
            )
        )
    )

    assert "<ns0:name>GE1/0/1</ns0:name>" in xml
    assert "<ns0:admin-state>up</ns0:admin-state>" in xml
    assert "<ns0:vlan-id>100</ns0:vlan-id>" in xml


def test_h3c_renderer_builds_access_interface_xml():
    xml = render_xml(
        H3cRenderer().render_interface_update(
            _Interface(
                name="GigabitEthernet1/0/13",
                admin_state=1,
                description=None,
                mode={"kind": "access", "access_vlan": 144},
            )
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().IFACE_NAMESPACE

    assert root.tag == f"{{{ns}}}Interface"
    assert root.find(f"{{{ns}}}IfIndex").text == "13"
    assert root.find(f"{{{ns}}}PVID").text == "144"


@pytest.mark.parametrize("renderer", [HuaweiRenderer()])
def test_vendor_renderer_normalizes_mixed_case_port_mode_kind(renderer):
    xml = renderer.render_edit_config(
        _DesiredState(
            vlans=[],
            interfaces=[
                _Interface(
                    name="GE1/0/1",
                    admin_state="up",
                    description=None,
                    mode={"kind": "Access", "access_vlan": 100},
                ),
                _Interface(
                    name="GE1/0/2",
                    admin_state="down",
                    description=None,
                    mode={
                        "kind": "Trunk",
                        "native_vlan": 100,
                        "allowed_vlans": [100, 200],
                    },
                ),
            ],
        )
    )

    root = ElementTree.fromstring(xml)
    assert root.find(f".//{{{renderer.IFACE_NAMESPACE}}}access") is not None
    assert root.find(f".//{{{renderer.IFACE_NAMESPACE}}}trunk") is not None
    admin_states = [
        node.text
        for node in root.findall(f".//{{{renderer.IFACE_NAMESPACE}}}admin-state")
    ]
    assert "down" in admin_states


def test_h3c_renderer_builds_real_comware_edit_config_document():
    xml = H3cRenderer().render_edit_config(
        _DesiredState(
            vlans=[_Vlan(vlan_id=144, name="tenant")],
            interfaces=[
                _Interface(
                    name="GigabitEthernet1/0/13",
                    admin_state=1,
                    description=None,
                    mode={"kind": "Access", "access_vlan": 144},
                ),
                _Interface(
                    name="Ten-GigabitEthernet1/0/21",
                    admin_state=1,
                    description=None,
                    mode={"kind": "Trunk", "allowed_vlans": [1004, 1003, 1, 1005]},
                ),
            ],
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().VLAN_NAMESPACE

    assert root.tag == f"{{{NETCONF_BASE_NAMESPACE}}}config"
    vlan = root.find(f".//{{{ns}}}VLANID")
    assert vlan.find(f"{{{ns}}}ID").text == "144"
    assert vlan.find(f"{{{ns}}}Name").text == "tenant"

    access = root.find(f".//{{{ns}}}AccessInterfaces/{{{ns}}}Interface")
    assert access.find(f"{{{ns}}}IfIndex").text == "13"
    assert access.find(f"{{{ns}}}PVID").text == "144"

    trunk = root.find(f".//{{{ns}}}TrunkInterfaces/{{{ns}}}Interface")
    assert trunk.find(f"{{{ns}}}IfIndex").text == "21"
    assert trunk.find(f"{{{ns}}}PermitVlanList").text == "1,1003-1005"
    assert root.find(f".//{{{ns}}}admin-state") is None


def test_h3c_renderer_builds_interface_description_edit_config_document():
    xml = H3cRenderer().render_edit_config(
        _DesiredState(
            vlans=[],
            interfaces=[
                _Interface(
                    name="GigabitEthernet1/0/13",
                    admin_state=1,
                    description="server access",
                    mode={"kind": "Access", "access_vlan": 144},
                )
            ],
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().IFACE_NAMESPACE

    ifmgr_interface = root.find(f".//{{{ns}}}Ifmgr/{{{ns}}}Interfaces/{{{ns}}}Interface")
    assert ifmgr_interface is not None
    assert ifmgr_interface.find(f"{{{ns}}}IfIndex").text == "13"
    assert ifmgr_interface.find(f"{{{ns}}}Description").text == "server access"
    access_interface = root.find(f".//{{{ns}}}AccessInterfaces/{{{ns}}}Interface")
    assert access_interface.find(f"{{{ns}}}PVID").text == "144"


def test_h3c_renderer_builds_ipv4_advanced_acl_edit_config_document():
    xml = H3cRenderer().render_edit_config(
        _DesiredState(
            vlans=[],
            interfaces=[],
            acls=[
                _Acl(
                    acl_id=3999,
                    description="ARIA isolated ACL",
                    rules=[
                        _AclRule(
                            sequence=10,
                            action="permit",
                            protocol="ip",
                            source=_AclEndpoint("192.0.2.1", "0.0.0.0"),
                            destination=_AclEndpoint("198.51.100.0", "0.0.0.255"),
                            description="allow test flow",
                        ),
                        _AclRule(
                            sequence=20,
                            action="deny",
                            protocol="tcp",
                            source=_AclEndpoint("192.0.2.0", "0.0.0.255"),
                            destination=_AclEndpoint("198.51.100.10", "0.0.0.0"),
                            destination_port_eq=443,
                        ),
                    ],
                )
            ],
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().ACL_NAMESPACE

    group = root.find(f".//{{{ns}}}ACL/{{{ns}}}Groups/{{{ns}}}Group")
    assert group.find(f"{{{ns}}}GroupType").text == "1"
    assert group.find(f"{{{ns}}}GroupID").text == "3999"
    assert group.find(f"{{{ns}}}Description").text == "ARIA isolated ACL"

    rules = root.findall(f".//{{{ns}}}ACL/{{{ns}}}IPv4AdvanceRules/{{{ns}}}Rule")
    assert [rule.find(f"{{{ns}}}RuleID").text for rule in rules] == ["10", "20"]
    assert rules[0].find(f"{{{ns}}}Action").text == "2"
    assert rules[0].find(f"{{{ns}}}ProtocolType").text == "256"
    assert rules[0].find(f"{{{ns}}}Description").text == "allow test flow"
    assert rules[0].find(f"{{{ns}}}SrcIPv4/{{{ns}}}SrcIPv4Addr").text == "192.0.2.1"
    assert rules[0].find(f"{{{ns}}}DstIPv4/{{{ns}}}DstIPv4Wildcard").text == "0.0.0.255"
    assert rules[1].find(f"{{{ns}}}Action").text == "1"
    assert rules[1].find(f"{{{ns}}}ProtocolType").text == "6"
    assert rules[1].find(f"{{{ns}}}DstPort/{{{ns}}}DstPortOp").text == "2"
    assert rules[1].find(f"{{{ns}}}DstPort/{{{ns}}}DstPortValue1").text == "443"


def test_h3c_renderer_builds_ipv4_basic_acl_edit_config_document():
    xml = H3cRenderer().render_edit_config(
        _DesiredState(
            vlans=[],
            interfaces=[],
            acls=[
                _Acl(
                    acl_id=2001,
                    kind="basic_ipv4",
                    description="ARIA basic ACL",
                    rules=[
                        _AclRule(
                            sequence=5,
                            action="permit",
                            protocol="ip",
                            source=_AclEndpoint("192.0.2.0", "0.0.0.255"),
                            description="allow redacted source",
                        )
                    ],
                )
            ],
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().ACL_NAMESPACE

    group = root.find(f".//{{{ns}}}ACL/{{{ns}}}Groups/{{{ns}}}Group")
    assert group.find(f"{{{ns}}}GroupID").text == "2001"
    assert group.find(f"{{{ns}}}Description").text == "ARIA basic ACL"

    basic_rules = root.findall(f".//{{{ns}}}ACL/{{{ns}}}IPv4BasicRules/{{{ns}}}Rule")
    advanced_rules = root.findall(f".//{{{ns}}}ACL/{{{ns}}}IPv4AdvanceRules/{{{ns}}}Rule")
    assert len(basic_rules) == 1
    assert advanced_rules == []
    assert basic_rules[0].find(f"{{{ns}}}RuleID").text == "5"
    assert basic_rules[0].find(f"{{{ns}}}Action").text == "2"
    assert basic_rules[0].find(f"{{{ns}}}ProtocolType") is None
    assert basic_rules[0].find(f"{{{ns}}}Description").text == "allow redacted source"
    assert (
        basic_rules[0].find(f"{{{ns}}}SrcIPv4/{{{ns}}}SrcIPv4Addr").text
        == "192.0.2.0"
    )
    assert basic_rules[0].find(f"{{{ns}}}DstIPv4") is None


def test_h3c_renderer_builds_interface_acl_binding_document():
    xml = H3cRenderer().render_edit_config(
        _DesiredState(
            vlans=[],
            interfaces=[],
            acls=[],
            acl_bindings=[
                _AclBinding(
                    interface_name="GigabitEthernet1/0/13",
                    direction="inbound",
                    acl_id=3999,
                )
            ],
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().ACL_NAMESPACE

    binding = root.find(f".//{{{ns}}}ACL/{{{ns}}}PfilterApply/{{{ns}}}Pfilter")
    assert binding is not None
    assert binding.find(f"{{{ns}}}AppObjType").text == "1"
    assert binding.find(f"{{{ns}}}AppObjIndex").text == "13"
    assert binding.find(f"{{{ns}}}AppDirection").text == "1"
    assert binding.find(f"{{{ns}}}AppAclType").text == "1"
    assert binding.find(f"{{{ns}}}AppAclGroup").text == "3999"


def test_h3c_renderer_builds_explicit_delete_document():
    xml = H3cRenderer().render_edit_config(
        _DesiredState(
            vlans=[],
            interfaces=[],
            acls=[],
            acl_bindings=[],
            delete_vlan_ids=[144],
            delete_acl_ids=[3999],
            delete_acl_bindings=[
                _AclBinding(
                    interface_name="GigabitEthernet1/0/13",
                    direction="inbound",
                    acl_id=3999,
                )
            ],
        )
    )
    root = ElementTree.fromstring(xml)
    ns = H3cRenderer().ACL_NAMESPACE
    operation_attr = f"{{{NETCONF_BASE_NAMESPACE}}}operation"

    vlan = root.find(f".//{{{ns}}}VLAN/{{{ns}}}VLANs/{{{ns}}}VLANID")
    assert vlan is not None
    assert vlan.attrib[operation_attr] == "delete"
    assert vlan.find(f"{{{ns}}}ID").text == "144"

    group = root.find(f".//{{{ns}}}ACL/{{{ns}}}Groups/{{{ns}}}Group")
    assert group is not None
    assert group.attrib[operation_attr] == "delete"
    assert group.find(f"{{{ns}}}GroupID").text == "3999"

    binding = root.find(f".//{{{ns}}}ACL/{{{ns}}}PfilterApply/{{{ns}}}Pfilter")
    assert binding is not None
    assert binding.attrib[operation_attr] == "delete"
    assert binding.find(f"{{{ns}}}AppObjIndex").text == "13"
    assert binding.find(f"{{{ns}}}AppAclGroup").text == "3999"


@pytest.mark.parametrize("renderer", [HuaweiRenderer()])
def test_vendor_renderer_builds_single_edit_config_document(renderer):
    xml = renderer.render_edit_config(
        _DesiredState(
            vlans=[
                _Vlan(vlan_id=100, name="prod", description="production vlan"),
                _Vlan(vlan_id=200, name="dev", description=None),
            ],
            interfaces=[
                _Interface(
                    name="GE1/0/1",
                    admin_state=1,
                    description="server uplink",
                    mode=SimpleNamespace(
                        kind=1,
                        access_vlan=100,
                        native_vlan=None,
                        allowed_vlans=[],
                    ),
                )
            ],
        )
    )

    root = ElementTree.fromstring(xml)
    assert root.tag == f"{{{NETCONF_BASE_NAMESPACE}}}config"
    vlan_nodes = root.findall(f".//{{{renderer.VLAN_NAMESPACE}}}vlan")
    assert [node.find(f"{{{renderer.VLAN_NAMESPACE}}}id").text for node in vlan_nodes] == [
        "100",
        "200",
    ]
    interface = root.find(f".//{{{renderer.IFACE_NAMESPACE}}}interface")
    assert interface is not None
    assert interface.find(f"{{{renderer.IFACE_NAMESPACE}}}admin-state").text == "up"
    assert interface.find(f".//{{{renderer.IFACE_NAMESPACE}}}vlan-id").text == "100"
    assert root.find(f".//{{{renderer.IFACE_NAMESPACE}}}vlan") is None
    assert root.find(f".//{{{renderer.VLAN_NAMESPACE}}}interface") is None


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_rejects_empty_edit_config_document(renderer):
    with pytest.raises(AdapterError) as exc:
        renderer.render_edit_config(_DesiredState(vlans=[], interfaces=[]))

    assert exc.value.code == "EMPTY_DESIRED_STATE"


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_rejects_unknown_port_mode(renderer):
    with pytest.raises(ValueError, match="unknown port mode"):
        renderer.render_interface_update(
            _Interface(
                name="GigabitEthernet1/0/13",
                admin_state="up",
                description=None,
                mode={"kind": "routed"},
            )
        )


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_rejects_invalid_vlan_id(renderer):
    with pytest.raises(ValueError, match="range 1..4094"):
        renderer.render_vlan_create(_Vlan(vlan_id=4095))


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_rejects_empty_interface_name(renderer):
    with pytest.raises(ValueError, match="name is required"):
        renderer.render_interface_update(
            _Interface(
                name=" ",
                admin_state="up",
                description=None,
                mode={"kind": "access", "access_vlan": 100},
            )
        )


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_rejects_duplicate_trunk_allowed_vlans(renderer):
    with pytest.raises(ValueError, match="duplicate allowed_vlans"):
        renderer.render_interface_update(
            _Interface(
                name="GigabitEthernet1/0/13",
                admin_state="up",
                description=None,
                mode={
                    "kind": "trunk",
                    "native_vlan": None,
                    "allowed_vlans": [100, 100],
                },
            )
        )


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_rejects_empty_trunk(renderer):
    with pytest.raises(ValueError, match="requires native_vlan or allowed_vlans"):
        renderer.render_interface_update(
            _Interface(
                name="GigabitEthernet1/0/13",
                admin_state="up",
                description=None,
                mode={
                    "kind": "trunk",
                    "native_vlan": None,
                    "allowed_vlans": [],
                },
            )
        )


def test_h3c_renderer_rejects_unverified_admin_down():
    with pytest.raises(ValueError, match="admin_state down is not supported"):
        H3cRenderer().render_interface_update(
            _Interface(
                name="GigabitEthernet1/0/13",
                admin_state="down",
                description=None,
                mode={"kind": "access", "access_vlan": 100},
            )
        )


def test_h3c_renderer_rejects_unverified_trunk_native_vlan():
    with pytest.raises(ValueError, match="trunk native_vlan is not supported"):
        H3cRenderer().render_interface_update(
            _Interface(
                name="Ten-GigabitEthernet1/0/21",
                admin_state=1,
                description=None,
                mode={"kind": "trunk", "native_vlan": 100, "allowed_vlans": [100]},
            )
        )


def test_h3c_renderer_rejects_named_acl():
    with pytest.raises(ValueError, match="ACL name is not supported"):
        H3cRenderer().render_edit_config(
            _DesiredState(
                vlans=[],
                interfaces=[],
                acls=[_Acl(acl_id=3999, name="existing-name")],
            )
        )


def test_h3c_renderer_rejects_acl_port_match_on_ip_protocol():
    with pytest.raises(ValueError, match="port matches require tcp or udp"):
        H3cRenderer().render_edit_config(
            _DesiredState(
                vlans=[],
                interfaces=[],
                acls=[
                    _Acl(
                        acl_id=3999,
                        rules=[
                            _AclRule(
                                sequence=10,
                                action="permit",
                                protocol="ip",
                                destination_port_eq=443,
                            )
                        ],
                    )
                ],
            )
        )
