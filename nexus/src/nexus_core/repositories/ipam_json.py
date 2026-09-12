from __future__ import annotations

import json
import os
import threading
from dataclasses import asdict
from pathlib import Path

from nexus_core.ports.ipam import IpamEntry


class JsonIpamRepository:
    def __init__(self, path: Path) -> None:
        self._path = path
        self._lock = threading.RLock()

    def list_entries(self) -> list[IpamEntry]:
        with self._lock:
            payload = self._read()
        return [IpamEntry(**item) for item in payload["entries"]]

    def get_entry(self, address: str) -> IpamEntry | None:
        return next((entry for entry in self.list_entries() if entry.address == address), None)

    def upsert_entry(self, entry: IpamEntry) -> IpamEntry:
        with self._lock:
            payload = self._read()
            entries = [item for item in payload["entries"] if item["address"] != entry.address]
            entries.append(asdict(entry))
            entries.sort(key=lambda item: tuple(int(part) for part in item["address"].split(".")))
            self._write({"version": 1, "entries": entries})
        return entry

    def delete_entry(self, address: str) -> IpamEntry:
        with self._lock:
            payload = self._read()
            existing = next((item for item in payload["entries"] if item["address"] == address), None)
            if existing is None:
                raise KeyError(address)
            payload["entries"] = [item for item in payload["entries"] if item["address"] != address]
            self._write(payload)
        return IpamEntry(**existing)

    def _read(self) -> dict[str, object]:
        if not self._path.exists():
            return {"version": 1, "entries": []}
        payload = json.loads(self._path.read_text(encoding="utf-8"))
        if payload.get("version") != 1 or not isinstance(payload.get("entries"), list):
            raise ValueError("unsupported IPAM inventory format")
        return payload

    def _write(self, payload: dict[str, object]) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self._path.with_suffix(self._path.suffix + f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(self._path)
