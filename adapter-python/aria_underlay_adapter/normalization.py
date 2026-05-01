from __future__ import annotations


def admin_state_to_text(value) -> str:
    if value is None or value == "" or value == 0:
        return "up"
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized in {"up", "down"}:
            return normalized
        return normalized
    if int(value) == 2:
        return "down"
    return "up"
