from dataclasses import dataclass

import pytest

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
    admin_state: str
    description: str | None
    mode: dict


def test_xml_renderer_escapes_text():
    xml = render_xml(XmlElement("description", children=["a & b < c"]))

    assert xml == "<description>a &amp; b &lt; c</description>"


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
