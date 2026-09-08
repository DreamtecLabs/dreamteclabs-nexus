from __future__ import annotations

import json
from dataclasses import asdict
from pathlib import Path

from nexus_core.ports.domains import DomainAuditEntry


class JsonlDomainAuditRepository:
    def __init__(self, path: Path) -> None:
        self._path = path

    def record(self, entry: DomainAuditEntry) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        with self._path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(asdict(entry), sort_keys=True) + "\n")

    def list_recent(self, limit: int = 25) -> tuple[DomainAuditEntry, ...]:
        if not self._path.exists():
            return ()
        limit = max(1, min(limit, 200))
        lines = self._path.read_text(encoding="utf-8").splitlines()[-limit:]
        entries: list[DomainAuditEntry] = []
        for line in reversed(lines):
            try:
                raw = json.loads(line)
                entries.append(DomainAuditEntry(
                    timestamp=str(raw["timestamp"]),
                    domain=str(raw["domain"]),
                    action=str(raw["action"]),
                    result=str(raw["result"]),
                    detail=str(raw["detail"]),
                    steps=tuple(raw.get("steps", [])),
                ))
            except (ValueError, TypeError, KeyError):
                continue
        return tuple(entries)
