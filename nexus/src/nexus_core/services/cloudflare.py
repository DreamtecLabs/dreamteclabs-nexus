from __future__ import annotations

import re
from datetime import datetime, timezone

from nexus_core.ports.domains import (
    CloudflareProvider,
    DnsRecord,
    DomainAuditEntry,
    DomainAuditRepository,
    TunnelIngressRule,
)

# Deliberately permissive: DNS names legitimately include underscore-prefixed
# labels (_dmarc, _domainkey, _sip._tcp for SRV) and a leading wildcard (*).
# This is a sanity check against garbage/injection, not full per-label RFC
# validation -- Cloudflare's own API is authoritative for correctness.
_DNS_NAME_RE = re.compile(r"^[A-Za-z0-9_*](?:[A-Za-z0-9_.*-]{0,251}[A-Za-z0-9_])?$")
_RECORD_ID_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
_SERVICE_RE = re.compile(r"^(https?://\S+|tcp://\S+|http_status:\d{3})$")
_VALID_RECORD_TYPES = frozenset({"A", "AAAA", "CNAME", "TXT", "MX", "NS", "SRV", "CAA"})


class CloudflareOperationsDisabled(RuntimeError):
    pass


class CloudflareService:
    def __init__(self, provider: CloudflareProvider, audit: DomainAuditRepository, *, operations_enabled: bool = False) -> None:
        self._provider = provider
        self._audit = audit
        self._operations_enabled = operations_enabled

    @property
    def operations_enabled(self) -> bool:
        return self._operations_enabled

    async def list_dns_records(self, zone_name: str) -> list[DnsRecord]:
        return await self._provider.list_dns_records(self._normalize_zone(zone_name))

    async def create_dns_record(
        self, zone_name: str, *, type: str, name: str, content: str, ttl: int = 1, proxied: bool = False, priority: int | None = None
    ) -> DnsRecord:
        self._require_enabled()
        zone = self._normalize_zone(zone_name)
        record_type = self._normalize_type(type)
        record_name = self._normalize_hostname(name)
        record_content = self._normalize_content(content)
        try:
            record = await self._provider.create_dns_record(
                zone, type=record_type, name=record_name, content=record_content, ttl=ttl, proxied=proxied, priority=priority
            )
        except RuntimeError as exc:
            self._audit_record(zone, "dns-create", "failed", f"{record_type} {record_name}: {exc}")
            raise
        self._audit_record(zone, "dns-create", "success", f"{record_type} {record_name} -> {record_content}")
        return record

    async def update_dns_record(
        self, zone_name: str, record_id: str, *, type: str, name: str, content: str, ttl: int = 1, proxied: bool = False, priority: int | None = None
    ) -> DnsRecord:
        self._require_enabled()
        zone = self._normalize_zone(zone_name)
        normalized_id = self._normalize_record_id(record_id)
        record_type = self._normalize_type(type)
        record_name = self._normalize_hostname(name)
        record_content = self._normalize_content(content)
        try:
            record = await self._provider.update_dns_record(
                zone, normalized_id, type=record_type, name=record_name, content=record_content, ttl=ttl, proxied=proxied, priority=priority
            )
        except RuntimeError as exc:
            self._audit_record(zone, "dns-update", "failed", f"{normalized_id}: {exc}")
            raise
        self._audit_record(zone, "dns-update", "success", f"{normalized_id} -> {record_type} {record_name} -> {record_content}")
        return record

    async def delete_dns_record(self, zone_name: str, record_id: str) -> None:
        self._require_enabled()
        zone = self._normalize_zone(zone_name)
        normalized_id = self._normalize_record_id(record_id)
        try:
            await self._provider.delete_dns_record(zone, normalized_id)
        except RuntimeError as exc:
            self._audit_record(zone, "dns-delete", "failed", f"{normalized_id}: {exc}")
            raise
        self._audit_record(zone, "dns-delete", "success", normalized_id)

    async def list_tunnel_ingress(self) -> list[TunnelIngressRule]:
        return await self._provider.list_tunnel_ingress()

    async def set_tunnel_ingress(self, rules: list[TunnelIngressRule]) -> list[TunnelIngressRule]:
        self._require_enabled()
        normalized = self._normalize_ingress(rules)
        try:
            await self._provider.set_tunnel_ingress(normalized)
        except RuntimeError as exc:
            self._audit_record("tunnel", "tunnel-ingress-update", "failed", str(exc))
            raise
        self._audit_record("tunnel", "tunnel-ingress-update", "success", f"{len(normalized)} rule(s)")
        return normalized

    def _audit_record(self, domain: str, action: str, result: str, detail: str) -> None:
        self._audit.record(
            DomainAuditEntry(
                timestamp=datetime.now(timezone.utc).isoformat(),
                domain=domain,
                action=action,
                result=result,
                detail=detail[:1000],
            )
        )

    def _require_enabled(self) -> None:
        if not self._operations_enabled:
            raise CloudflareOperationsDisabled("Cloudflare DNS/tunnel mutations are disabled by configuration")

    @staticmethod
    def _normalize_zone(value: str) -> str:
        zone = value.strip().lower().rstrip(".")
        if not zone or len(zone) > 253:
            raise ValueError("zone name is required")
        return zone

    @staticmethod
    def _normalize_type(value: str) -> str:
        record_type = value.strip().upper()
        if record_type not in _VALID_RECORD_TYPES:
            raise ValueError(f"unsupported record type: {value}")
        return record_type

    @staticmethod
    def _normalize_hostname(value: str) -> str:
        hostname = value.strip().lower().rstrip(".")
        if not hostname or len(hostname) > 253 or not _DNS_NAME_RE.fullmatch(hostname):
            raise ValueError(f"invalid DNS record name: {value}")
        return hostname

    @staticmethod
    def _normalize_content(value: str) -> str:
        content = value.strip()
        if not content or len(content) > 2048:
            raise ValueError("record content must be 1-2048 characters")
        return content

    @staticmethod
    def _normalize_record_id(value: str) -> str:
        record_id = value.strip()
        if not _RECORD_ID_RE.fullmatch(record_id):
            raise ValueError("invalid record id")
        return record_id

    @staticmethod
    def _normalize_ingress(rules: list[TunnelIngressRule]) -> list[TunnelIngressRule]:
        if not rules:
            raise ValueError("ingress list cannot be empty")
        normalized: list[TunnelIngressRule] = []
        for index, rule in enumerate(rules):
            is_last = index == len(rules) - 1
            service = rule.service.strip()
            if not _SERVICE_RE.fullmatch(service):
                raise ValueError(f"invalid service target: {rule.service}")
            hostname = rule.hostname.strip().lower().rstrip(".") if rule.hostname else None
            if hostname:
                if len(hostname) > 253 or not _DNS_NAME_RE.fullmatch(hostname):
                    raise ValueError(f"invalid hostname: {rule.hostname}")
            elif not is_last:
                raise ValueError("only the last ingress rule may omit a hostname (catch-all)")
            normalized.append(TunnelIngressRule(hostname=hostname, service=service, no_tls_verify=rule.no_tls_verify))
        if normalized[-1].hostname is not None:
            normalized.append(TunnelIngressRule(hostname=None, service="http_status:404"))
        return normalized
