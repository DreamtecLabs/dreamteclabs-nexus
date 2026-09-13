from __future__ import annotations

import datetime as dt
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken

from nexus_core.ports.vault import VaultAuditEntry, VaultRepository, VaultSecretMeta, VaultSecretRecord


def ensure_vault_key(path: Path) -> bytes:
    """Generate the vault's own encryption key on first use -- same lazy,
    on-disk-once pattern as SshBootstrapService.ensure_keypair()."""
    if path.exists():
        return path.read_bytes()
    path.parent.mkdir(parents=True, exist_ok=True)
    key = Fernet.generate_key()
    path.write_bytes(key)
    path.chmod(0o600)
    return key


def _now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


class VaultService:
    """Encrypted-at-rest storage for keys, certificates and other secrets Nexus
    otherwise has nowhere safe to put. Note this is not an access-control
    boundary by itself -- Nexus Core has no login of its own, so revealing a
    secret through the UI is only as protected as reaching Nexus at all is."""

    def __init__(self, repository: VaultRepository, cipher: Fernet, audit_repository=None) -> None:
        self._repository = repository
        self._cipher = cipher
        self._audit = audit_repository

    def list_secrets(self) -> list[VaultSecretMeta]:
        return sorted(self._repository.list_secrets(), key=lambda item: item.name.casefold())

    def list_audit(self, limit: int = 25) -> tuple[VaultAuditEntry, ...]:
        return self._audit.list_recent(limit) if self._audit is not None else ()

    def create_secret(self, *, name: str, value: str, type: str = "generic", notes: str = "") -> VaultSecretMeta:
        name = name.strip()
        if not name:
            raise ValueError("name is required")
        if not value:
            raise ValueError("value is required")
        if self._repository.get_secret(name) is not None:
            raise ValueError(f"a secret named '{name}' already exists")
        now = _now()
        record = VaultSecretRecord(name=name, type=type.strip() or "generic", notes=notes.strip(), created_at=now, updated_at=now, ciphertext=self._encrypt(value))
        self._repository.upsert_secret(record)
        self._log(name, "create")
        return self._meta(record)

    def update_secret(self, name: str, *, value: str | None = None, notes: str | None = None) -> VaultSecretMeta:
        existing = self._repository.get_secret(name)
        if existing is None:
            raise KeyError(name)
        record = VaultSecretRecord(
            name=existing.name,
            type=existing.type,
            notes=existing.notes if notes is None else notes.strip(),
            created_at=existing.created_at,
            updated_at=_now(),
            ciphertext=self._encrypt(value) if value else existing.ciphertext,
        )
        self._repository.upsert_secret(record)
        self._log(name, "update")
        return self._meta(record)

    def delete_secret(self, name: str) -> None:
        self._repository.delete_secret(name)
        self._log(name, "delete")

    def reveal_secret(self, name: str) -> str:
        record = self._repository.get_secret(name)
        if record is None:
            raise KeyError(name)
        try:
            value = self._cipher.decrypt(record.ciphertext.encode("ascii")).decode("utf-8")
        except InvalidToken as exc:
            raise RuntimeError("stored secret could not be decrypted -- the vault key may have changed") from exc
        self._log(name, "reveal")
        return value

    def _encrypt(self, value: str) -> str:
        return self._cipher.encrypt(value.encode("utf-8")).decode("ascii")

    def _log(self, name: str, action: str) -> None:
        if self._audit is not None:
            self._audit.record(VaultAuditEntry(timestamp=_now(), name=name, action=action))

    @staticmethod
    def _meta(record: VaultSecretRecord) -> VaultSecretMeta:
        return VaultSecretMeta(name=record.name, type=record.type, notes=record.notes, created_at=record.created_at, updated_at=record.updated_at)
