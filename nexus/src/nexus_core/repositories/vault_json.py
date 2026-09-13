from __future__ import annotations

import json
import os
import threading
from dataclasses import asdict
from pathlib import Path

from nexus_core.ports.vault import VaultSecretMeta, VaultSecretRecord


class JsonVaultRepository:
    """Pure storage: ciphertext blobs in, ciphertext blobs out. Encryption itself
    is VaultService's job -- this repository never sees a plaintext secret."""

    def __init__(self, path: Path) -> None:
        self._path = path
        self._lock = threading.RLock()

    def list_secrets(self) -> list[VaultSecretMeta]:
        with self._lock:
            payload = self._read()
        return [VaultSecretMeta(name=item["name"], type=item["type"], notes=item["notes"], created_at=item["created_at"], updated_at=item["updated_at"], filename=item.get("filename")) for item in payload["secrets"].values()]

    def get_secret(self, name: str) -> VaultSecretRecord | None:
        with self._lock:
            raw = self._read()["secrets"].get(name)
        if raw is None:
            return None
        raw.setdefault("filename", None)
        return VaultSecretRecord(**raw)

    def upsert_secret(self, record: VaultSecretRecord) -> None:
        with self._lock:
            payload = self._read()
            payload["secrets"][record.name] = asdict(record)
            self._write(payload)

    def delete_secret(self, name: str) -> None:
        with self._lock:
            payload = self._read()
            if name not in payload["secrets"]:
                raise KeyError(name)
            del payload["secrets"][name]
            self._write(payload)

    def _read(self) -> dict[str, object]:
        if not self._path.exists():
            return {"version": 1, "secrets": {}}
        payload = json.loads(self._path.read_text(encoding="utf-8"))
        if payload.get("version") != 1 or not isinstance(payload.get("secrets"), dict):
            raise ValueError("unsupported vault format")
        return payload

    def _write(self, payload: dict[str, object]) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self._path.with_suffix(self._path.suffix + f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(self._path)
        self._path.chmod(0o600)
