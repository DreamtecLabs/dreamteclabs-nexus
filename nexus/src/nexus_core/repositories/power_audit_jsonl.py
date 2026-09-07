from __future__ import annotations

import json
from dataclasses import asdict
from pathlib import Path

from nexus_core.ports.infrastructure import PowerAuditEntry


class JsonlPowerAuditRepository:
    def __init__(self, path: Path) -> None:
        self._path = path

    def record(self, entry: PowerAuditEntry) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        with self._path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(asdict(entry), separators=(",", ":"), ensure_ascii=False))
            handle.write("\n")

    def list_recent(self, limit: int = 25) -> tuple[PowerAuditEntry, ...]:
        limit = max(1, min(int(limit), 200))
        if not self._path.exists():
            return ()
        entries: list[PowerAuditEntry] = []
        for line in self._path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            try:
                raw = json.loads(line)
                entries.append(PowerAuditEntry(**raw))
            except (json.JSONDecodeError, TypeError):
                continue
        return tuple(reversed(entries[-limit:]))
