from __future__ import annotations

from aria_underlay_adapter.renderers.common import RendererNamespaceProfile
from aria_underlay_adapter.renderers.common import StructuredSkeletonRenderer


HUAWEI_VRP8_SKELETON_PROFILE = RendererNamespaceProfile(
    vendor="huawei",
    profile_name="vrp8-skeleton",
    vlan_namespace="urn:aria:underlay:renderer:huawei:vrp8:vlan:skeleton",
    interface_namespace="urn:aria:underlay:renderer:huawei:vrp8:interface:skeleton",
    production_ready=False,
)


class HuaweiRenderer(StructuredSkeletonRenderer):
    """Structured XML renderer skeleton for Huawei VRP.

    The profile is intentionally not production-ready until the exact Huawei
    YANG namespace and field mapping are verified on target devices.
    """

    profile = HUAWEI_VRP8_SKELETON_PROFILE
