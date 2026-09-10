from __future__ import annotations

import json
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.domains import DnsRecord, TunnelIngressRule
from nexus_core.providers.cloudflare import CloudflareApiProvider
from nexus_core.repositories.domain_audit_jsonl import JsonlDomainAuditRepository
from nexus_core.services.cloudflare import CloudflareOperationsDisabled, CloudflareService


class FakeCloudflareProvider:
    def __init__(self, records: list[DnsRecord] | None = None, ingress: list[TunnelIngressRule] | None = None) -> None:
        self.records = list(records or [])
        self.ingress = list(ingress or [TunnelIngressRule(hostname=None, service="http_status:404")])
        self.created: list[tuple[str, dict[str, object]]] = []
        self.updated: list[tuple[str, str, dict[str, object]]] = []
        self.deleted: list[tuple[str, str]] = []
        self.set_ingress_calls: list[list[TunnelIngressRule]] = []

    async def list_dns_records(self, zone_name: str) -> list[DnsRecord]:
        return self.records

    async def create_dns_record(self, zone_name: str, **kwargs: object) -> DnsRecord:
        self.created.append((zone_name, kwargs))
        return DnsRecord(id="new-1", type=str(kwargs["type"]), name=str(kwargs["name"]), content=str(kwargs["content"]), ttl=1, proxied=bool(kwargs.get("proxied", False)))

    async def update_dns_record(self, zone_name: str, record_id: str, **kwargs: object) -> DnsRecord:
        self.updated.append((zone_name, record_id, kwargs))
        return DnsRecord(id=record_id, type=str(kwargs["type"]), name=str(kwargs["name"]), content=str(kwargs["content"]), ttl=1, proxied=bool(kwargs.get("proxied", False)))

    async def delete_dns_record(self, zone_name: str, record_id: str) -> None:
        self.deleted.append((zone_name, record_id))

    async def list_tunnel_ingress(self) -> list[TunnelIngressRule]:
        return self.ingress

    async def set_tunnel_ingress(self, rules: list[TunnelIngressRule]) -> None:
        self.set_ingress_calls.append(rules)
        self.ingress = rules


@pytest.mark.asyncio
async def test_cloudflare_provider_lists_dns_records_across_pages() -> None:
    calls = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request.url.path + "?" + str(request.url.params))
        if "/zones" == request.url.path:
            return httpx.Response(200, json={"success": True, "result": [{"id": "zone-1", "name": "example.com"}]})
        page = request.url.params.get("page")
        if page == "1":
            return httpx.Response(
                200,
                json={
                    "success": True,
                    "result": [{"id": "r1", "type": "A", "name": "example.com", "content": "1.2.3.4", "ttl": 1, "proxied": True}],
                    "result_info": {"page": 1, "total_pages": 2},
                },
            )
        return httpx.Response(
            200,
            json={
                "success": True,
                "result": [{"id": "r2", "type": "TXT", "name": "example.com", "content": "v=spf1 ~all", "ttl": 300, "proxied": False}],
                "result_info": {"page": 2, "total_pages": 2},
            },
        )

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    records = await provider.list_dns_records("example.com")
    assert [r.id for r in records] == ["r1", "r2"]
    assert records[0].proxied is True
    assert records[1].content == "v=spf1 ~all"


@pytest.mark.asyncio
async def test_cloudflare_provider_sends_structured_data_for_srv_and_caa() -> None:
    captured: list[dict[str, object]] = []

    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/zones":
            return httpx.Response(200, json={"success": True, "result": [{"id": "zone-1"}]})
        body = json.loads(request.content)
        captured.append(body)
        return httpx.Response(
            200,
            json={"success": True, "result": {"id": "r-srv", "type": body["type"], "name": body["name"], "content": "", "ttl": 1, "proxied": False, "data": body.get("data")}},
        )

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    srv_data = {"service": "_sip", "proto": "_tcp", "name": "example.com", "priority": 10, "weight": 5, "port": 5060, "target": "sipserver.example.com"}
    record = await provider.create_dns_record("example.com", type="SRV", name="_sip._tcp.example.com", data=srv_data)
    assert "content" not in captured[0]
    assert captured[0]["data"] == srv_data
    assert record.data == srv_data

    caa_data = {"flags": 0, "tag": "issue", "value": "letsencrypt.org"}
    await provider.create_dns_record("example.com", type="CAA", name="example.com", data=caa_data)
    assert captured[1]["data"] == caa_data
    assert "content" not in captured[1]


@pytest.mark.asyncio
async def test_cloudflare_provider_reports_errors_from_response_body() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/zones":
            return httpx.Response(200, json={"success": True, "result": [{"id": "zone-1"}]})
        return httpx.Response(400, json={"success": False, "errors": [{"code": 81057, "message": "Record already exists."}]})

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    with pytest.raises(RuntimeError, match="81057: Record already exists"):
        await provider.create_dns_record("example.com", type="A", name="example.com", content="1.2.3.4")


@pytest.mark.asyncio
async def test_cloudflare_provider_roundtrips_advanced_ingress_fields() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        if request.method == "GET":
            return httpx.Response(
                200,
                json={
                    "success": True,
                    "result": {
                        "config": {
                            "ingress": [
                                {
                                    "hostname": "app.example.com",
                                    "path": "/api/*",
                                    "service": "http://192.168.0.10:80",
                                    "originRequest": {"noTLSVerify": True, "httpHostHeader": "internal.example", "originServerName": "internal.example", "connectTimeout": "15s"},
                                },
                                {"service": "http_status:404"},
                            ]
                        }
                    },
                },
            )
        captured["body"] = json.loads(request.content)
        return httpx.Response(200, json={"success": True, "result": {}})

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    rules = await provider.list_tunnel_ingress()
    assert rules[0].path == "/api/*"
    assert rules[0].http_host_header == "internal.example"
    assert rules[0].origin_server_name == "internal.example"
    assert rules[0].connect_timeout_seconds == 15

    await provider.set_tunnel_ingress(rules)
    saved = captured["body"]["config"]["ingress"][0]
    assert saved["path"] == "/api/*"
    assert saved["originRequest"]["httpHostHeader"] == "internal.example"
    assert saved["originRequest"]["connectTimeout"] == "15s"


@pytest.mark.asyncio
async def test_cloudflare_provider_appends_catch_all_when_missing() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        if request.method == "GET":
            return httpx.Response(200, json={"success": True, "result": {"config": {"ingress": []}}})
        captured["body"] = json.loads(request.content)
        return httpx.Response(200, json={"success": True, "result": {}})

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    await provider.set_tunnel_ingress([TunnelIngressRule(hostname="app.example.com", service="http://192.168.0.10:80")])
    ingress = captured["body"]["config"]["ingress"]
    assert ingress[-1] == {"service": "http_status:404"}
    assert ingress[0]["hostname"] == "app.example.com"


@pytest.mark.asyncio
async def test_cloudflare_service_gates_mutations_and_validates_input(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider()
    locked = CloudflareService(provider, audit, operations_enabled=False)
    with pytest.raises(CloudflareOperationsDisabled):
        await locked.create_dns_record("example.com", type="A", name="www.example.com", content="1.2.3.4")

    service = CloudflareService(provider, audit, operations_enabled=True)
    with pytest.raises(ValueError):
        await service.create_dns_record("example.com", type="BOGUS", name="www.example.com", content="1.2.3.4")
    with pytest.raises(ValueError):
        await service.create_dns_record("example.com", type="A", name="bad host name!", content="1.2.3.4")

    record = await service.create_dns_record("example.com", type="a", name="WWW.example.com", content="1.2.3.4", ttl=1, proxied=False, priority=None)
    assert record.type == "A"
    assert provider.created[0][1]["name"] == "www.example.com"
    assert audit.list_recent(1)[0].action == "dns-create"
    assert audit.list_recent(1)[0].result == "success"


@pytest.mark.asyncio
async def test_cloudflare_service_requires_and_validates_structured_data_for_srv_caa(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider()
    service = CloudflareService(provider, audit, operations_enabled=True)

    # SRV without data at all
    with pytest.raises(ValueError, match="require structured data"):
        await service.create_dns_record("example.com", type="SRV", name="_sip._tcp.example.com", content="")

    # SRV missing required fields
    with pytest.raises(ValueError, match="missing fields"):
        await service.create_dns_record("example.com", type="SRV", name="_sip._tcp.example.com", data={"service": "_sip", "proto": "_tcp"})

    # SRV with a bad port
    with pytest.raises(ValueError):
        await service.create_dns_record(
            "example.com",
            type="SRV",
            name="_sip._tcp.example.com",
            data={"service": "_sip", "proto": "_tcp", "name": "example.com", "priority": 10, "weight": 5, "port": 99999, "target": "sip.example.com"},
        )

    # valid SRV
    record = await service.create_dns_record(
        "example.com",
        type="SRV",
        name="_sip._tcp.example.com",
        data={"service": "_sip", "proto": "_tcp", "name": "example.com", "priority": 10, "weight": 5, "port": 5060, "target": "sip.example.com"},
    )
    assert record is not None
    assert provider.created[0][1]["data"]["port"] == 5060
    assert provider.created[0][1]["content"] == ""

    # CAA with invalid tag
    with pytest.raises(ValueError, match="invalid CAA tag"):
        await service.create_dns_record("example.com", type="CAA", name="example.com", data={"tag": "bogus", "value": "letsencrypt.org"})

    # valid CAA
    await service.create_dns_record("example.com", type="CAA", name="example.com", data={"tag": "issue", "value": "letsencrypt.org"})
    assert provider.created[1][1]["data"] == {"flags": 0, "tag": "issue", "value": "letsencrypt.org"}


@pytest.mark.asyncio
async def test_cloudflare_service_normalizes_tunnel_ingress(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider()
    service = CloudflareService(provider, audit, operations_enabled=True)

    with pytest.raises(ValueError):
        await service.set_tunnel_ingress([])

    with pytest.raises(ValueError):
        # a non-last rule may not omit its hostname
        await service.set_tunnel_ingress(
            [TunnelIngressRule(hostname=None, service="http_status:404"), TunnelIngressRule(hostname="a.example.com", service="http://x:80")]
        )

    saved = await service.set_tunnel_ingress([TunnelIngressRule(hostname="a.example.com", service="http://x:80")])
    assert saved[-1].hostname is None
    assert provider.set_ingress_calls[0][-1].service == "http_status:404"

    with pytest.raises(ValueError):
        await service.set_tunnel_ingress([TunnelIngressRule(hostname="a.example.com", service="http://x:80", connect_timeout_seconds=999)])

    with pytest.raises(ValueError):
        await service.set_tunnel_ingress([TunnelIngressRule(hostname="a.example.com", service="http://x:80", http_host_header="bad header!")])

    saved_advanced = await service.set_tunnel_ingress(
        [TunnelIngressRule(hostname="a.example.com", service="http://x:80", path="/api/*", http_host_header="internal.example", origin_server_name="internal.example", connect_timeout_seconds=30)]
    )
    assert saved_advanced[0].path == "/api/*"
    assert saved_advanced[0].connect_timeout_seconds == 30


def test_cloudflare_dns_page_and_tunnel_page_render(tmp_path: Path) -> None:
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_DOMAINS_OPERATIONS_ENABLED=False)
    app = create_app(settings)
    provider = FakeCloudflareProvider(
        records=[DnsRecord(id="r1", type="A", name="example.com", content="1.2.3.4", ttl=1, proxied=True)],
        ingress=[TunnelIngressRule(hostname="app.example.com", service="http://192.168.0.10:80"), TunnelIngressRule(hostname=None, service="http_status:404")],
    )
    app.state.cloudflare_service = CloudflareService(provider, JsonlDomainAuditRepository(tmp_path / "audit.jsonl"), operations_enabled=False)

    with TestClient(app) as client:
        dns_page = client.get("/domains/dns", params={"zone": "example.com"})
        assert dns_page.status_code == 200
        assert "example.com" in dns_page.text
        assert "1.2.3.4" in dns_page.text

        tunnel_page = client.get("/domains/tunnel")
        assert tunnel_page.status_code == 200
        assert "app.example.com" in tunnel_page.text
        assert "(catch-all)" in tunnel_page.text

        blocked = client.post("/api/v1/cloudflare/zones/example.com/dns-records", json={"type": "A", "name": "www.example.com", "content": "1.2.3.4"})
        assert blocked.status_code == 403


def test_cloudflare_dns_api_crud_when_enabled(tmp_path: Path) -> None:
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_DOMAINS_OPERATIONS_ENABLED=True)
    app = create_app(settings)
    provider = FakeCloudflareProvider(records=[DnsRecord(id="r1", type="A", name="example.com", content="1.2.3.4", ttl=1, proxied=False)])
    app.state.cloudflare_service = CloudflareService(provider, JsonlDomainAuditRepository(tmp_path / "audit.jsonl"), operations_enabled=True)

    with TestClient(app) as client:
        listed = client.get("/api/v1/cloudflare/zones/example.com/dns-records")
        assert listed.status_code == 200
        assert listed.json()["count"] == 1

        created = client.post("/api/v1/cloudflare/zones/example.com/dns-records", json={"type": "TXT", "name": "example.com", "content": "hello"})
        assert created.status_code == 200
        assert created.json()["type"] == "TXT"

        updated = client.put("/api/v1/cloudflare/zones/example.com/dns-records/r1", json={"type": "A", "name": "example.com", "content": "5.6.7.8"})
        assert updated.status_code == 200
        assert provider.updated[0][2]["content"] == "5.6.7.8"

        deleted = client.delete("/api/v1/cloudflare/zones/example.com/dns-records/r1")
        assert deleted.status_code == 200
        assert provider.deleted == [("example.com", "r1")]

        ingress_saved = client.put("/api/v1/cloudflare/tunnel/ingress", json={"rules": [{"hostname": "app.example.com", "service": "http://x:80"}]})
        assert ingress_saved.status_code == 200
        assert ingress_saved.json()["rules"][-1]["hostname"] is None
