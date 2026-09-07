from __future__ import annotations

import json
import os
import threading
from dataclasses import asdict
from pathlib import Path

from nexus_core.ports.monitoring import MonitoringTarget


class JsonMonitoringRepository:
    def __init__(self, path: Path) -> None:
        self._path = path
        self._lock = threading.RLock()

    def list_targets(self) -> list[MonitoringTarget]:
        with self._lock:
            payload = self._read()
        return [MonitoringTarget(**item) for item in payload["targets"]]

    def get_target(self, target_id: str) -> MonitoringTarget | None:
        return next((target for target in self.list_targets() if target.id == target_id), None)

    def upsert_target(self, target: MonitoringTarget) -> MonitoringTarget:
        with self._lock:
            payload = self._read()
            targets = [item for item in payload["targets"] if item["id"] != target.id]
            targets.append(asdict(target))
            targets.sort(key=lambda item: (item["name"].casefold(), item["id"]))
            self._write({"version": 1, "targets": targets})
        return target

    def delete_target(self, target_id: str) -> MonitoringTarget:
        with self._lock:
            payload = self._read()
            existing = next((item for item in payload["targets"] if item["id"] == target_id), None)
            if existing is None:
                raise KeyError(target_id)
            payload["targets"] = [item for item in payload["targets"] if item["id"] != target_id]
            self._write(payload)
        return MonitoringTarget(**existing)

    def _read(self) -> dict[str, object]:
        if not self._path.exists():
            return {"version": 1, "targets": []}
        payload = json.loads(self._path.read_text(encoding="utf-8"))
        if payload.get("version") != 1 or not isinstance(payload.get("targets"), list):
            raise ValueError("unsupported monitoring inventory format")
        return payload

    def _write(self, payload: dict[str, object]) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self._path.with_suffix(self._path.suffix + f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(self._path)
