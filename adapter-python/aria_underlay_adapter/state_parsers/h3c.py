from __future__ import annotations

from aria_underlay_adapter.state_parsers.skeleton import SkeletonStateParser
from aria_underlay_adapter.state_parsers.skeleton import StateParserProfile


H3C_COMWARE7_STATE_PARSER_PROFILE = StateParserProfile(
    vendor="h3c",
    profile_name="comware7-state-skeleton",
    production_ready=False,
)


class H3cStateParser(SkeletonStateParser):
    """Running state parser skeleton for H3C Comware.

    The parser is intentionally not production-ready until real device
    get-config XML has been captured and mapped.
    """

    profile = H3C_COMWARE7_STATE_PARSER_PROFILE
