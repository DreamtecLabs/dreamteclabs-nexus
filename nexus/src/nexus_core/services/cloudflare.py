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
# Cloudflare Tunnel ingress accepts several service schemes beyond plain
# HTTP(S)/TCP -- ssh/rdp/smb power its browser-rendered Infrastructure Access,
# and unix: addresses a local socket. Reject only obvious garbage; Cloudflare's
# own API is authoritative for whether a given target is actually valid.
_SERVICE_RE = re.compile(r"^(https?://\S+|tcp://\S+|ssh://\S+|rdp://\S+|smb://\S+|unix:\S+|http_status:\d{3})$")
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

    async def upsert_tunnel_rule(
        self,
        *,
        original_hostname: str | None,
        original_path: str | None,
        hostname: str | None,
        service: str,
        path: str | None = None,
        no_tls_verify: bool = False,
        http_host_header: str | None = None,
        origin_server_name: str | None = None,
        connect_timeout_seconds: int | None = None,
    ) -> None:
        """Add or edit exactly one ingress rule, in place, without touching any
        other rule. Fetches the live config fresh right before writing (never
        the possibly-stale page load) and passes every other entry through
        untouched -- so a rule this code doesn't fully model (an exotic
        service scheme, extra originRequest fields) can never be silently
        dropped or corrupted just because a neighboring rule was edited."""
        self._require_enabled()
        normalized_service = self._normalize_service(service)

        if not hostname or not hostname.strip():
            # The trailing catch-all has no hostname and is always unique --
            # editing it just means swapping its service. But this branch is
            # also reached if the caller was editing a NAMED rule and cleared
            # its hostname, so the original rule (identified by
            # original_hostname/original_path) has to be dropped too, not
            # just any pre-existing catch-all -- otherwise the old named rule
            # is left behind alongside a freshly appended catch-all.
            original_hostname_norm = self._normalize_ingress_hostname(original_hostname) if original_hostname else None
            original_path_norm = self._normalize_ingress_path(original_path) if original_path else None
            try:
                ingress = await self._provider.get_tunnel_ingress_raw()
                named = [
                    item
                    for item in ingress
                    if item.get("hostname") and not (original_hostname_norm and self._matches(item, original_hostname_norm, original_path_norm))
                ]
                named.append({"service": normalized_service})
                await self._provider.put_tunnel_ingress_raw(named)
            except RuntimeError as exc:
                self._audit_record("tunnel", "tunnel-ingress-upsert", "failed", f"(catch-all): {exc}")
                raise
            self._audit_record("tunnel", "tunnel-ingress-upsert", "success", f"(catch-all) -> {normalized_service}")
            return

        normalized_hostname = self._normalize_ingress_hostname(hostname)
        normalized_path = self._normalize_ingress_path(path)
        normalized_host_header = self._normalize_ingress_hostlike(http_host_header, "HTTP host header")
        normalized_sni = self._normalize_ingress_hostlike(origin_server_name, "origin server name")
        if connect_timeout_seconds is not None and not (1 <= connect_timeout_seconds <= 300):
            raise ValueError("connect timeout must be between 1 and 300 seconds")

        entry: dict[str, object] = {"hostname": normalized_hostname, "service": normalized_service}
        if normalized_path:
            entry["path"] = normalized_path
        origin_request: dict[str, object] = {}
        if no_tls_verify:
            origin_request["noTLSVerify"] = True
        if normalized_host_header:
            origin_request["httpHostHeader"] = normalized_host_header
        if normalized_sni:
            origin_request["originServerName"] = normalized_sni
        if connect_timeout_seconds is not None:
            origin_request["connectTimeout"] = f"{connect_timeout_seconds}s"
        if origin_request:
            entry["originRequest"] = origin_request

        original_hostname_norm = self._normalize_ingress_hostname(original_hostname) if original_hostname else None
        original_path_norm = self._normalize_ingress_path(original_path) if original_path else None

        try:
            ingress = await self._provider.get_tunnel_ingress_raw()
            if original_hostname_norm:
                remaining = [item for item in ingress if not self._matches(item, original_hostname_norm, original_path_norm)]
            else:
                remaining = list(ingress)
            catchall_index = next((i for i, item in enumerate(remaining) if not item.get("hostname")), len(remaining))
            remaining.insert(catchall_index, entry)
            if not any(not item.get("hostname") for item in remaining):
                remaining.append({"service": "http_status:404"})
            await self._provider.put_tunnel_ingress_raw(remaining)
        except RuntimeError as exc:
            self._audit_record("tunnel", "tunnel-ingress-upsert", "failed", f"{normalized_hostname}: {exc}")
            raise
        self._audit_record("tunnel", "tunnel-ingress-upsert", "success", f"{normalized_hostname} -> {normalized_service}")

    async def delete_tunnel_rule(self, hostname: str, path: str | None = None) -> None:
        self._require_enabled()
        normalized_hostname = self._normalize_ingress_hostname(hostname)
        normalized_path = self._normalize_ingress_path(path)
        try:
            ingress = await self._provider.get_tunnel_ingress_raw()
            remaining = [item for item in ingress if not self._matches(item, normalized_hostname, normalized_path)]
            if not any(not item.get("hostname") for item in remaining):
                remaining.append({"service": "http_status:404"})
            await self._provider.put_tunnel_ingress_raw(remaining)
        except RuntimeError as exc:
            self._audit_record("tunnel", "tunnel-ingress-delete", "failed", f"{normalized_hostname}: {exc}")
            raise
        self._audit_record("tunnel", "tunnel-ingress-delete", "success", normalized_hostname)

    @staticmethod
    def _matches(item: dict[str, object], hostname: str, path: str | None) -> bool:
        return item.get("hostname") == hostname and (item.get("path") or None) == path

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
    def _normalize_service(value: str) -> str:
        service = value.strip()
        if not _SERVICE_RE.fullmatch(service):
            raise ValueError(f"invalid service target: {value}")
        return service

    @staticmethod
    def _normalize_ingress_hostname(value: str) -> str:
        hostname = value.strip().lower().rstrip(".")
        if not hostname or len(hostname) > 253 or not _DNS_NAME_RE.fullmatch(hostname):
            raise ValueError(f"invalid hostname: {value}")
        return hostname

    @staticmethod
    def _normalize_ingress_path(value: str | None) -> str | None:
        if not value or not value.strip():
            return None
        path = value.strip()
        if len(path) > 512 or "\n" in path:
            raise ValueError(f"invalid path pattern: {value}")
        return path

    @staticmethod
    def _normalize_ingress_hostlike(value: str | None, label: str) -> str | None:
        if not value or not value.strip():
            return None
        cleaned = value.strip()
        if len(cleaned) > 253 or not _DNS_NAME_RE.fullmatch(cleaned.lower()):
            raise ValueError(f"invalid {label}: {value}")
        return cleaned
