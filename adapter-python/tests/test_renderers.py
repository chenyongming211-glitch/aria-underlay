from dataclasses import dataclass
from types import SimpleNamespace

import pytest

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.renderers.h3c import H3cRenderer
from aria_underlay_adapter.renderers.huawei import HuaweiRenderer
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


@pytest.mark.parametrize("renderer", [HuaweiRenderer(), H3cRenderer()])
def test_vendor_renderer_skeletons_are_not_production_ready(renderer):
    assert renderer.production_ready is False


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

    assert 'operation="delete"' in xml
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
