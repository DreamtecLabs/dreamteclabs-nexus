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


@dataclass(frozen=True, slots=True)
class DnsRecord:
    id: str
    type: str
    name: str
    content: str
    ttl: int
    proxied: bool
    priority: int | None = None


@dataclass(frozen=True, slots=True)
class TunnelIngressRule:
    """One entry of a Cloudflare Tunnel's ingress list. `hostname` is None for the
    trailing catch-all rule Cloudflare requires every ingress list to end with."""

    hostname: str | None
    service: str
    path: str | None = None
    no_tls_verify: bool = False
    http_host_header: str | None = None
    origin_server_name: str | None = None
    connect_timeout_seconds: int | None = None


class CloudflareProvider(Protocol):
    async def list_dns_records(self, zone_name: str) -> list[DnsRecord]: ...

    async def create_dns_record(
        self,
        zone_name: str,
        *,
        type: str,
        name: str,
        content: str,
        ttl: int = 1,
        proxied: bool = False,
        priority: int | None = None,
    ) -> DnsRecord: ...

    async def update_dns_record(
        self,
        zone_name: str,
        record_id: str,
        *,
        type: str,
        name: str,
        content: str,
        ttl: int = 1,
        proxied: bool = False,
        priority: int | None = None,
    ) -> DnsRecord: ...

    async def delete_dns_record(self, zone_name: str, record_id: str) -> None: ...

    async def list_tunnel_ingress(self) -> list[TunnelIngressRule]: ...

    async def set_tunnel_ingress(self, rules: list[TunnelIngressRule]) -> None: ...
