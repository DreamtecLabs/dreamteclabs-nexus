from __future__ import annotations

from typing import Any

import httpx

from nexus_core.ports.domains import DnsRecord, TunnelIngressRule


class CloudflareApiProvider:
    def __init__(
        self,
        *,
        api_base: str,
        api_token: str,
        account_id: str,
        tunnel_id: str,
        timeout_seconds: float = 10.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._api_base = api_base.rstrip("/")
        self._api_token = api_token
        self._account_id = account_id
        self._tunnel_id = tunnel_id
        self._timeout_seconds = timeout_seconds
        self._transport = transport
        self._zone_cache: dict[str, str] = {}

    async def _request(self, method: str, path: str, **kwargs: object) -> dict[str, Any]:
        headers = {"Authorization": f"Bearer {self._api_token}", "Content-Type": "application/json"}
        try:
            async with httpx.AsyncClient(
                base_url=self._api_base,
                headers=headers,
                timeout=self._timeout_seconds,
                transport=self._transport,
            ) as client:
                response = await client.request(method, path, **kwargs)
        except httpx.HTTPError as exc:
            raise RuntimeError(f"Cloudflare request failed: {type(exc).__name__}") from exc
        try:
            data = response.json()
        except ValueError as exc:
            raise RuntimeError("Cloudflare returned invalid JSON") from exc
        if not isinstance(data, dict):
            raise RuntimeError("Cloudflare returned an unexpected response")
        if not response.is_success or data.get("success") is not True:
            errors = data.get("errors")
            if isinstance(errors, list) and errors:
                message = "; ".join(
                    f"{(item.get('code') if isinstance(item, dict) else 'unknown')}: {(item.get('message') if isinstance(item, dict) else 'unknown error')}"
                    for item in errors
                )
            else:
                message = f"HTTP {response.status_code}"
            raise RuntimeError(f"Cloudflare {method} {path} failed: {message}")
        return data

    async def _zone_id(self, zone_name: str) -> str:
        cached = self._zone_cache.get(zone_name)
        if cached:
            return cached
        data = await self._request("GET", "/zones", params={"name": zone_name, "status": "active"})
        results = data.get("result")
        zone_id = None
        if isinstance(results, list) and results:
            first = results[0]
            if isinstance(first, dict):
                zone_id = first.get("id")
        if not isinstance(zone_id, str) or not zone_id:
            raise RuntimeError(f"Cloudflare has no active zone for {zone_name}")
        self._zone_cache[zone_name] = zone_id
        return zone_id

    @staticmethod
    def _parse_record(raw: object) -> DnsRecord | None:
        if not isinstance(raw, dict):
            return None
        record_id, record_type, name = raw.get("id"), raw.get("type"), raw.get("name")
        content = raw.get("content")
        if not all(isinstance(value, str) for value in (record_id, record_type, name)) or not isinstance(content, (str, type(None))):
            return None
        ttl = raw.get("ttl")
        priority = raw.get("priority")
        data = raw.get("data")
        return DnsRecord(
            id=record_id,
            type=record_type,
            name=name,
            content=content or "",
            ttl=int(ttl) if isinstance(ttl, (int, float)) else 1,
            proxied=bool(raw.get("proxied", False)),
            priority=int(priority) if isinstance(priority, (int, float)) else None,
            data=data if isinstance(data, dict) else None,
        )

    async def list_dns_records(self, zone_name: str) -> list[DnsRecord]:
        zone_id = await self._zone_id(zone_name)
        records: list[DnsRecord] = []
        page = 1
        while True:
            data = await self._request("GET", f"/zones/{zone_id}/dns_records", params={"page": page, "per_page": 100})
            result = data.get("result")
            if isinstance(result, list):
                records.extend(record for raw in result if (record := self._parse_record(raw)) is not None)
            info = data.get("result_info")
            total_pages = info.get("total_pages") if isinstance(info, dict) else None
            if not isinstance(total_pages, (int, float)) or page >= total_pages:
                break
            page += 1
        return records

    @staticmethod
    def _record_payload(
        *, type: str, name: str, content: str, ttl: int, proxied: bool, priority: int | None, data: dict[str, object] | None
    ) -> dict[str, object]:
        # SRV and CAA don't take a flat `content` string -- Cloudflare requires
        # their fields (service/proto/priority/weight/port/target, or
        # flags/tag/value) nested under `data` instead.
        if type in ("SRV", "CAA") and data is not None:
            return {"type": type, "name": name, "data": data, "ttl": ttl, "proxied": False}
        payload: dict[str, object] = {"type": type, "name": name, "content": content, "ttl": ttl, "proxied": proxied}
        if type == "MX":
            payload["priority"] = priority if priority is not None else 10
            payload["proxied"] = False
        return payload

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
        zone_id = await self._zone_id(zone_name)
        payload = self._record_payload(type=type, name=name, content=content, ttl=ttl, proxied=proxied, priority=priority, data=data)
        result = await self._request("POST", f"/zones/{zone_id}/dns_records", json=payload)
        record = self._parse_record(result.get("result"))
        if record is None:
            raise RuntimeError("Cloudflare did not return the created record")
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
        zone_id = await self._zone_id(zone_name)
        payload = self._record_payload(type=type, name=name, content=content, ttl=ttl, proxied=proxied, priority=priority, data=data)
        result = await self._request("PUT", f"/zones/{zone_id}/dns_records/{record_id}", json=payload)
        record = self._parse_record(result.get("result"))
        if record is None:
            raise RuntimeError("Cloudflare did not return the updated record")
        return record

    async def delete_dns_record(self, zone_name: str, record_id: str) -> None:
        zone_id = await self._zone_id(zone_name)
        await self._request("DELETE", f"/zones/{zone_id}/dns_records/{record_id}")

    @staticmethod
    def _parse_connect_timeout(origin_request: dict[str, object]) -> int | None:
        value = origin_request.get("connectTimeout")
        if isinstance(value, str) and value.endswith("s") and value[:-1].isdigit():
            return int(value[:-1])
        if isinstance(value, (int, float)):
            return int(value)
        return None

    @classmethod
    def _parse_ingress(cls, raw: object) -> list[TunnelIngressRule]:
        rules: list[TunnelIngressRule] = []
        if not isinstance(raw, list):
            return rules
        for item in raw:
            if not isinstance(item, dict):
                continue
            service = item.get("service")
            if not isinstance(service, str):
                continue
            hostname = item.get("hostname")
            path = item.get("path")
            origin_request = item.get("originRequest") if isinstance(item.get("originRequest"), dict) else {}
            http_host_header = origin_request.get("httpHostHeader")
            origin_server_name = origin_request.get("originServerName")
            rules.append(
                TunnelIngressRule(
                    hostname=hostname if isinstance(hostname, str) else None,
                    service=service,
                    path=path if isinstance(path, str) else None,
                    no_tls_verify=bool(origin_request.get("noTLSVerify")),
                    http_host_header=http_host_header if isinstance(http_host_header, str) else None,
                    origin_server_name=origin_server_name if isinstance(origin_server_name, str) else None,
                    connect_timeout_seconds=cls._parse_connect_timeout(origin_request),
                )
            )
        return rules

    async def list_tunnel_ingress(self) -> list[TunnelIngressRule]:
        data = await self._request("GET", f"/accounts/{self._account_id}/cfd_tunnel/{self._tunnel_id}/configurations")
        result = data.get("result")
        config = result.get("config") if isinstance(result, dict) else None
        ingress = config.get("ingress") if isinstance(config, dict) else None
        return self._parse_ingress(ingress)

    async def set_tunnel_ingress(self, rules: list[TunnelIngressRule]) -> None:
        ingress: list[dict[str, object]] = []
        for rule in rules:
            entry: dict[str, object] = {"service": rule.service}
            if rule.hostname:
                entry["hostname"] = rule.hostname
                if rule.path:
                    entry["path"] = rule.path
                origin_request: dict[str, object] = {"noTLSVerify": rule.no_tls_verify}
                if rule.http_host_header:
                    origin_request["httpHostHeader"] = rule.http_host_header
                if rule.origin_server_name:
                    origin_request["originServerName"] = rule.origin_server_name
                if rule.connect_timeout_seconds is not None:
                    origin_request["connectTimeout"] = f"{rule.connect_timeout_seconds}s"
                entry["originRequest"] = origin_request
            ingress.append(entry)
        if not ingress or ingress[-1].get("hostname") is not None:
            ingress.append({"service": "http_status:404"})
        await self._request(
            "PUT",
            f"/accounts/{self._account_id}/cfd_tunnel/{self._tunnel_id}/configurations",
            json={"config": {"ingress": ingress}},
        )


class UnconfiguredCloudflareProvider:
    async def list_dns_records(self, zone_name: str) -> list[DnsRecord]:
        raise RuntimeError("NEXUS_CF_API_TOKEN is not configured")

    async def create_dns_record(
        self, zone_name: str, *, type: str, name: str, content: str, ttl: int = 1, proxied: bool = False, priority: int | None = None
    ) -> DnsRecord:
        raise RuntimeError("NEXUS_CF_API_TOKEN is not configured")

    async def update_dns_record(
        self, zone_name: str, record_id: str, *, type: str, name: str, content: str, ttl: int = 1, proxied: bool = False, priority: int | None = None
    ) -> DnsRecord:
        raise RuntimeError("NEXUS_CF_API_TOKEN is not configured")

    async def delete_dns_record(self, zone_name: str, record_id: str) -> None:
        raise RuntimeError("NEXUS_CF_API_TOKEN is not configured")

    async def list_tunnel_ingress(self) -> list[TunnelIngressRule]:
        raise RuntimeError("NEXUS_CF_API_TOKEN is not configured")

    async def set_tunnel_ingress(self, rules: list[TunnelIngressRule]) -> None:
        raise RuntimeError("NEXUS_CF_API_TOKEN is not configured")
