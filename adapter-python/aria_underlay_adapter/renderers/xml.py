from __future__ import annotations

from dataclasses import dataclass, field
from typing import Union
from xml.etree import ElementTree


NETCONF_BASE_NAMESPACE = "urn:ietf:params:xml:ns:netconf:base:1.0"
XmlChild = Union["XmlElement", str]


@dataclass(frozen=True)
class XmlElement:
    name: str
    namespace: str | None = None
    attributes: dict[str, str] = field(default_factory=dict)
    children: list[XmlChild] = field(default_factory=list)


def render_xml(element: XmlElement) -> str:
    return ElementTree.tostring(_to_element(element), encoding="unicode")


def _to_element(node: XmlElement) -> ElementTree.Element:
    element = ElementTree.Element(_qualified_name(node.name, node.namespace), node.attributes)
    for child in node.children:
        if isinstance(child, XmlElement):
            element.append(_to_element(child))
        else:
            if len(element):
                tail_target = element[-1]
                tail_target.tail = (tail_target.tail or "") + child
            else:
                element.text = (element.text or "") + child
    return element


def _qualified_name(name: str, namespace: str | None) -> str:
    if namespace is None:
        return name
    return f"{{{namespace}}}{name}"


def qualified_attr(name: str, namespace: str | None) -> str:
    return _qualified_name(name, namespace)
