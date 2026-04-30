from __future__ import annotations

from dataclasses import dataclass

from aria_underlay_adapter.errors import AdapterError


@dataclass(frozen=True)
class StateParserProfile:
    vendor: str
    profile_name: str
    production_ready: bool = False
    fixture_verified: bool = False


class SkeletonStateParser:
    profile: StateParserProfile

    @property
    def production_ready(self) -> bool:
        return self.profile.production_ready

    def parse_running(self, xml: str, scope=None) -> dict:
        raise AdapterError(
            code="NETCONF_STATE_PARSER_NOT_IMPLEMENTED",
            message=(
                f"NETCONF running state parser for {self.profile.vendor} "
                "is not implemented"
            ),
            normalized_error="state parser not implemented",
            raw_error_summary=(
                f"vendor={self.profile.vendor}, profile={self.profile.profile_name}"
            ),
            retryable=False,
        )
