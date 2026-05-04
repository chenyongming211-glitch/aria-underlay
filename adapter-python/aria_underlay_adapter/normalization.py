from __future__ import annotations


def admin_state_to_text(value) -> str:
    if value is None or value == "" or value == 0:
        return "up"
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized in {"up", "down"}:
            return normalized
        raise ValueError(f"unknown admin state: {value}")
    try:
        numeric = int(value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"unknown admin state: {value}") from exc
    if numeric == 2:
        return "down"
    if numeric in {0, 1}:
        return "up"
    raise ValueError(f"unknown admin state: {value}")
