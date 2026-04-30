from __future__ import annotations

from aria_underlay_adapter.renderers.common import RendererNamespaceProfile
from aria_underlay_adapter.renderers.common import StructuredSkeletonRenderer


H3C_COMWARE7_SKELETON_PROFILE = RendererNamespaceProfile(
    vendor="h3c",
    profile_name="comware7-skeleton",
    vlan_namespace="urn:aria:underlay:renderer:h3c:comware7:vlan:skeleton",
    interface_namespace="urn:aria:underlay:renderer:h3c:comware7:interface:skeleton",
    production_ready=False,
)


class H3cRenderer(StructuredSkeletonRenderer):
    """Structured XML renderer skeleton for H3C Comware.

    The profile is intentionally not production-ready until the exact H3C
    YANG namespace and field mapping are verified on target devices.
    """

    profile = H3C_COMWARE7_SKELETON_PROFILE
