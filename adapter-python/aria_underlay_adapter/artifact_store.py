from __future__ import annotations

import json
from pathlib import Path


class ArtifactStore:
    def __init__(self, root: str):
        self._root = Path(root).resolve()

    def save_json(self, device_id: str, tx_id: str, name: str, payload: dict) -> Path:
        directory = self._root / device_id / tx_id
        path = directory / name
        resolved = path.resolve()
        if not str(resolved).startswith(str(self._root)):
            raise ValueError("path traversal detected")
        directory.mkdir(parents=True, exist_ok=True)
        resolved.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
        return resolved

