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
_SRV_REQUIRED_FIELDS = ("service", "proto", "name", "priority", "weight", "port", "target")
_CAA_TAGS = frozenset({"issue", "issuewild", "iodef"})


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
        self,
        zone_name: str,
        *,
        type: str,
        name: str,
        content: str = "",
        ttl: int = 1,
        proxied: bool = False,
        priority: int | None = None,
        data: dict[str, object] | None = None,
    ) -> DnsRecord:
        self._require_enabled()
        zone = self._normalize_zone(zone_name)
        record_type = self._normalize_type(type)
        record_name = self._normalize_hostname(name)
        record_data = self._normalize_record_data(record_type, data)
        record_content = "" if record_data is not None else self._normalize_content(content)
        try:
            record = await self._provider.create_dns_record(
                zone, type=record_type, name=record_name, content=record_content, ttl=ttl, proxied=proxied, priority=priority, data=record_data
            )
        except RuntimeError as exc:
            self._audit_record(zone, "dns-create", "failed", f"{record_type} {record_name}: {exc}")
            raise
        self._audit_record(zone, "dns-create", "success", f"{record_type} {record_name} -> {record_data or record_content}")
        return record

    async def update_dns_record(
        self,
        zone_name: str,
        record_id: str,
        *,
        type: str,
        name: str,
        content: str = "",
        ttl: int = 1,
        proxied: bool = False,
        priority: int | None = None,
        data: dict[str, object] | None = None,
    ) -> DnsRecord:
        self._require_enabled()
        zone = self._normalize_zone(zone_name)
        normalized_id = self._normalize_record_id(record_id)
        record_type = self._normalize_type(type)
        record_name = self._normalize_hostname(name)
        record_data = self._normalize_record_data(record_type, data)
        record_content = "" if record_data is not None else self._normalize_content(content)
        try:
            record = await self._provider.update_dns_record(
                zone, normalized_id, type=record_type, name=record_name, content=record_content, ttl=ttl, proxied=proxied, priority=priority, data=record_data
            )
        except RuntimeError as exc:
            self._audit_record(zone, "dns-update", "failed", f"{normalized_id}: {exc}")
            raise
        self._audit_record(zone, "dns-update", "success", f"{normalized_id} -> {record_type} {record_name} -> {record_data or record_content}")
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
    def _normalize_record_data(record_type: str, raw: dict[str, object] | None) -> dict[str, object] | None:
        if record_type not in ("SRV", "CAA"):
            return None
        if not isinstance(raw, dict):
            raise ValueError(f"{record_type} records require structured data")

        if record_type == "SRV":
            missing = [key for key in _SRV_REQUIRED_FIELDS if key not in raw]
            if missing:
                raise ValueError(f"SRV record data missing fields: {', '.join(missing)}")
            service = str(raw["service"]).strip()
            proto = str(raw["proto"]).strip()
            name = str(raw["name"]).strip().lower().rstrip(".")
            target = str(raw["target"]).strip().lower().rstrip(".")
            if not service.startswith("_") or not proto.startswith("_"):
                raise ValueError("SRV service and proto must start with an underscore (e.g. _sip, _tcp)")
            if not name or not _DNS_NAME_RE.fullmatch(name):
                raise ValueError(f"invalid SRV name: {raw['name']}")
            if not target or (target != "." and not _DNS_NAME_RE.fullmatch(target)):
                raise ValueError(f"invalid SRV target: {raw['target']}")
            try:
                priority, weight, port = int(raw["priority"]), int(raw["weight"]), int(raw["port"])
            except (TypeError, ValueError) as exc:
                raise ValueError("SRV priority/weight/port must be integers") from exc
            if not (0 <= priority <= 65535 and 0 <= weight <= 65535 and 1 <= port <= 65535):
                raise ValueError("SRV priority/weight must be 0-65535 and port 1-65535")
            return {"service": service, "proto": proto, "name": name, "priority": priority, "weight": weight, "port": port, "target": target}

        # CAA
        if "tag" not in raw or "value" not in raw:
            raise ValueError("CAA record data requires tag and value")
        tag = str(raw["tag"]).strip().lower()
        if tag not in _CAA_TAGS:
            raise ValueError(f"invalid CAA tag: {raw['tag']} (must be issue, issuewild or iodef)")
        value = str(raw["value"]).strip()
        if not value or len(value) > 512:
            raise ValueError("CAA value must be 1-512 characters")
        try:
            flags = int(raw.get("flags", 0))
        except (TypeError, ValueError) as exc:
            raise ValueError("CAA flags must be an integer") from exc
        return {"flags": flags, "tag": tag, "value": value}

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

            path = rule.path.strip() if rule.path else None
            if path and (len(path) > 512 or "\n" in path):
                raise ValueError(f"invalid path pattern: {rule.path}")

            http_host_header = rule.http_host_header.strip() if rule.http_host_header else None
            if http_host_header and (len(http_host_header) > 253 or not _DNS_NAME_RE.fullmatch(http_host_header.lower())):
                raise ValueError(f"invalid HTTP host header: {rule.http_host_header}")

            origin_server_name = rule.origin_server_name.strip() if rule.origin_server_name else None
            if origin_server_name and (len(origin_server_name) > 253 or not _DNS_NAME_RE.fullmatch(origin_server_name.lower())):
                raise ValueError(f"invalid origin server name: {rule.origin_server_name}")

            connect_timeout = rule.connect_timeout_seconds
            if connect_timeout is not None and not (1 <= connect_timeout <= 300):
                raise ValueError("connect timeout must be between 1 and 300 seconds")

            normalized.append(
                TunnelIngressRule(
                    hostname=hostname,
                    service=service,
                    path=path,
                    no_tls_verify=rule.no_tls_verify,
                    http_host_header=http_host_header,
                    origin_server_name=origin_server_name,
                    connect_timeout_seconds=connect_timeout,
                )
            )
        if normalized[-1].hostname is not None:
            normalized.append(TunnelIngressRule(hostname=None, service="http_status:404"))
        return normalized
