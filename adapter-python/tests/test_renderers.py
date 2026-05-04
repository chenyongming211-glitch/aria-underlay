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


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_skeletons_are_not_production_ready(renderer):
    assert renderer.production_ready is False
    assert renderer.profile.production_ready is False
    assert renderer.profile.vendor in {"huawei", "h3c"}
    assert renderer.profile.profile_name.endswith("-skeleton")
    assert renderer.VLAN_NAMESPACE.endswith(":skeleton")
    assert renderer.IFACE_NAMESPACE.endswith(":skeleton")


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


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_builds_vlan_create_xml(renderer):
    xml = render_xml(
        renderer.render_vlan_create(
            _Vlan(vlan_id=100, name="prod", description="production vlan")
        )
    )

    assert "<ns0:id>100</ns0:id>" in xml
    assert "<ns0:name>prod</ns0:name>" in xml
    assert "<ns0:description>production vlan</ns0:description>" in xml


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_builds_vlan_delete_xml(renderer):
    xml = render_xml(renderer.render_vlan_delete(100))
    root = ElementTree.fromstring(xml)

    assert 'operation="delete"' in xml
    assert root.attrib[f"{{{NETCONF_BASE_NAMESPACE}}}operation"] == "delete"
    assert "<ns0:id>100</ns0:id>" in xml


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
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


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
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


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
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

    assert xml.startswith("<config")
    assert xml.count("<ns0:vlan") == 2
    assert xml.count("<ns1:interface") == 1
    assert "<ns0:id>100</ns0:id>" in xml
    assert "<ns0:id>200</ns0:id>" in xml
    assert "<ns1:admin-state>up</ns1:admin-state>" in xml
    assert "<ns1:vlan-id>100</ns1:vlan-id>" in xml

    root = ElementTree.fromstring(xml)
    assert root.find(f".//{{{renderer.VLAN_NAMESPACE}}}vlan") is not None
    assert root.find(f".//{{{renderer.IFACE_NAMESPACE}}}interface") is not None
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
                name="GE1/0/1",
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
                name="GE1/0/1",
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
                name="GE1/0/1",
                admin_state="up",
                description=None,
                mode={
                    "kind": "trunk",
                    "native_vlan": None,
                    "allowed_vlans": [],
                },
            )
        )
