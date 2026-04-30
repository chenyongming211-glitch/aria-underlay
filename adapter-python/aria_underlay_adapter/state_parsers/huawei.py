from __future__ import annotations

from aria_underlay_adapter.state_parsers.skeleton import SkeletonStateParser
from aria_underlay_adapter.state_parsers.skeleton import StateParserProfile


HUAWEI_VRP8_STATE_PARSER_PROFILE = StateParserProfile(
    vendor="huawei",
    profile_name="vrp8-state-skeleton",
    production_ready=False,
)


class HuaweiStateParser(SkeletonStateParser):
    """Running state parser skeleton for Huawei VRP.

    The parser is intentionally not production-ready until real device
    get-config XML has been captured and mapped.
    """

    profile = HUAWEI_VRP8_STATE_PARSER_PROFILE
