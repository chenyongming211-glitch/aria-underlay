from __future__ import annotations

from aria_underlay_adapter.state_parsers.common import FixtureStateParser
from aria_underlay_adapter.state_parsers.skeleton import StateParserProfile


HUAWEI_VRP8_STATE_PARSER_PROFILE = StateParserProfile(
    vendor="huawei",
    profile_name="vrp8-state-fixture",
    production_ready=False,
    fixture_verified=True,
)


class HuaweiStateParser(FixtureStateParser):
    """Fixture-verified running state parser for Huawei VRP.

    The parser is intentionally not production-ready until real device XML
    has been captured and verified.
    """

    profile = HUAWEI_VRP8_STATE_PARSER_PROFILE
