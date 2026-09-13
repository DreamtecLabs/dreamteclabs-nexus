from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True, slots=True)
class VaultSecretMeta:
    name: str
    type: str
    notes: str
    created_at: str
    updated_at: str


@dataclass(frozen=True, slots=True)
class VaultSecretRecord(VaultSecretMeta):
    ciphertext: str


@dataclass(frozen=True, slots=True)
class VaultAuditEntry:
    timestamp: str
    name: str
    action: str  # "create" | "update" | "reveal" | "delete"


class VaultRepository(Protocol):
    def list_secrets(self) -> list[VaultSecretMeta]: ...

    def get_secret(self, name: str) -> VaultSecretRecord | None: ...

    def upsert_secret(self, record: VaultSecretRecord) -> None: ...

    def delete_secret(self, name: str) -> None: ...
