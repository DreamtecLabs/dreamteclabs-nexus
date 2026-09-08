from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol


DOMAIN_MODES = frozenset({"managed", "existing", "discovered", "pending"})


@dataclass(frozen=True, slots=True)
class DomainRecord:
    name: str
    mail: bool = False
    webmail: bool = False
    ddns: bool = False
    tunnel: bool = False
    configuration_mode: str = "discovered"
    hestia_user: str = "admin"


@dataclass(frozen=True, slots=True)
class DomainCheck:
    key: str
    label: str
    ok: bool
    detail: str


@dataclass(frozen=True, slots=True)
class DomainValidation:
    domain: str
    healthy: bool
    checks: tuple[DomainCheck, ...]


@dataclass(frozen=True, slots=True)
class DomainOperationResult:
    domain: str
    action: str
    ok: bool
    steps: tuple[str, ...] = ()
    detail: str | None = None


@dataclass(frozen=True, slots=True)
class DomainAuditEntry:
    timestamp: str
    domain: str
    action: str
    result: str
    detail: str
    steps: tuple[str, ...] = ()


class DomainRepository(Protocol):
    def list_domains(self) -> tuple[DomainRecord, ...]: ...
    def get_domain(self, name: str) -> DomainRecord | None: ...
    def upsert_domain(self, domain: DomainRecord) -> DomainRecord: ...


class DomainDiagnosticsProvider(Protocol):
    async def validate(self, domain: DomainRecord) -> DomainValidation: ...


class DomainOrchestratorProvider(Protocol):
    async def reconcile(self, domain: DomainRecord, *, migrate: bool) -> DomainOperationResult: ...


class DomainAuditRepository(Protocol):
    def record(self, entry: DomainAuditEntry) -> None: ...
    def list_recent(self, limit: int = 25) -> tuple[DomainAuditEntry, ...]: ...
