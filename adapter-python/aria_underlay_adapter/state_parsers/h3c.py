from __future__ import annotations

from aria_underlay_adapter.state_parsers.common import FixtureStateParser
from aria_underlay_adapter.state_parsers.skeleton import StateParserProfile


H3C_COMWARE7_STATE_PARSER_PROFILE = StateParserProfile(
    vendor="h3c",
    profile_name="comware7-state-fixture",
    production_ready=False,
    fixture_verified=True,
)


class H3cStateParser(FixtureStateParser):
    """Fixture-verified running state parser for H3C Comware.

    The parser is intentionally not production-ready until real device XML
    has been captured and verified.
    """

    profile = H3C_COMWARE7_STATE_PARSER_PROFILE
