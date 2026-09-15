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
    def __init__(
        self,
        records: list[DnsRecord] | None = None,
        ingress: list[dict[str, object]] | None = None,
        *,
        known_zones: set[str] | None = None,
        tunnel_id: str = "tunnel-abc",
    ) -> None:
        self.records = list(records or [])
        self.raw_ingress: list[dict[str, object]] = list(ingress or [{"service": "http_status:404"}])
        self.created: list[tuple[str, dict[str, object]]] = []
        self.updated: list[tuple[str, str, dict[str, object]]] = []
        self.deleted: list[tuple[str, str]] = []
        self.put_ingress_calls: list[list[dict[str, object]]] = []
        # None means every zone name is treated as valid (old behavior, used by
        # tests that don't care about zone resolution). Set it to constrain
        # which candidate zone names list_dns_records accepts, mirroring
        # Cloudflare's real "no active zone with that name" error.
        self.known_zones = known_zones
        self._tunnel_id = tunnel_id

    async def list_dns_records(self, zone_name: str) -> list[DnsRecord]:
        if self.known_zones is not None and zone_name not in self.known_zones:
            raise RuntimeError(f"Cloudflare has no active zone for {zone_name}")
        return self.records

    def tunnel_target(self) -> str:
        return f"{self._tunnel_id}.cfargotunnel.com"

    async def create_dns_record(self, zone_name: str, **kwargs: object) -> DnsRecord:
        self.created.append((zone_name, kwargs))
        return DnsRecord(id="new-1", type=str(kwargs["type"]), name=str(kwargs["name"]), content=str(kwargs["content"]), ttl=1, proxied=bool(kwargs.get("proxied", False)))

    async def update_dns_record(self, zone_name: str, record_id: str, **kwargs: object) -> DnsRecord:
        self.updated.append((zone_name, record_id, kwargs))
        return DnsRecord(id=record_id, type=str(kwargs["type"]), name=str(kwargs["name"]), content=str(kwargs["content"]), ttl=1, proxied=bool(kwargs.get("proxied", False)))

    async def delete_dns_record(self, zone_name: str, record_id: str) -> None:
        self.deleted.append((zone_name, record_id))

    async def list_tunnel_ingress(self) -> list[TunnelIngressRule]:
        return CloudflareApiProvider._parse_ingress(self.raw_ingress)

    async def get_tunnel_ingress_raw(self) -> list[dict[str, object]]:
        return list(self.raw_ingress)

    async def put_tunnel_ingress_raw(self, ingress: list[dict[str, object]]) -> None:
        self.put_ingress_calls.append(ingress)
        self.raw_ingress = ingress


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
async def test_cloudflare_provider_parses_advanced_ingress_fields() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
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

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    rules = await provider.list_tunnel_ingress()
    assert rules[0].path == "/api/*"
    assert rules[0].http_host_header == "internal.example"
    assert rules[0].origin_server_name == "internal.example"
    assert rules[0].connect_timeout_seconds == 15


@pytest.mark.asyncio
async def test_cloudflare_provider_raw_get_and_put_roundtrip() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        if request.method == "GET":
            return httpx.Response(
                200,
                json={"success": True, "result": {"config": {"ingress": [{"hostname": "app.example.com", "service": "http://192.168.0.10:80"}, {"service": "http_status:404"}]}}},
            )
        captured["body"] = json.loads(request.content)
        return httpx.Response(200, json={"success": True, "result": {}})

    provider = CloudflareApiProvider(
        api_base="http://cf.test", api_token="secret", account_id="acct", tunnel_id="tunnel", transport=httpx.MockTransport(handler)
    )
    raw = await provider.get_tunnel_ingress_raw()
    assert raw == [{"hostname": "app.example.com", "service": "http://192.168.0.10:80"}, {"service": "http_status:404"}]

    await provider.put_tunnel_ingress_raw(raw)
    assert captured["body"] == {"config": {"ingress": raw}}


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
async def test_cloudflare_service_upsert_adds_new_rule_and_keeps_catch_all(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider()
    service = CloudflareService(provider, audit, operations_enabled=True)

    await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="a.example.com", service="http://x:80")
    saved = provider.put_ingress_calls[0]
    assert [item.get("hostname") for item in saved] == ["a.example.com", None]
    assert saved[-1] == {"service": "http_status:404"}
    # [0] is the DNS auto-create that follows a successful upsert (list_recent is newest-first).
    ingress_entry = audit.list_recent(2)[1]
    assert ingress_entry.action == "tunnel-ingress-upsert"
    assert ingress_entry.result == "success"


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_only_touches_the_edited_rule(tmp_path: Path) -> None:
    """The core guarantee behind the per-rule save flow: editing one hostname
    must never reshape or drop any other rule already on the tunnel, even one
    Nexus doesn't fully model (an unrecognized field, an exotic scheme)."""
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    untouched_exotic = {"hostname": "legacy.example.com", "service": "rdp://192.168.0.5:3389", "someFutureField": "keep-me"}
    provider = FakeCloudflareProvider(
        ingress=[
            {"hostname": "a.example.com", "service": "http://old:80"},
            untouched_exotic,
            {"service": "http_status:404"},
        ]
    )
    service = CloudflareService(provider, audit, operations_enabled=True)

    await service.upsert_tunnel_rule(original_hostname="a.example.com", original_path=None, hostname="a.example.com", service="http://new:80")

    saved = provider.put_ingress_calls[0]
    assert saved[0] == untouched_exotic  # passed through byte-for-byte, never re-validated or reshaped
    assert saved[1]["service"] == "http://new:80"
    assert saved[-1] == {"service": "http_status:404"}


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_disambiguates_duplicate_hostnames_by_path(tmp_path: Path) -> None:
    """A tunnel can legitimately route the same hostname to different services
    by path -- editing one path must not touch the other."""
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider(
        ingress=[
            {"hostname": "family.example.com", "service": "http://api:3001", "path": "/api/v1/*"},
            {"hostname": "family.example.com", "service": "http://web:3000"},
            {"service": "http_status:404"},
        ]
    )
    service = CloudflareService(provider, audit, operations_enabled=True)

    await service.upsert_tunnel_rule(
        original_hostname="family.example.com", original_path=None, hostname="family.example.com", service="http://web:3999"
    )

    saved = provider.put_ingress_calls[0]
    api_rule = next(item for item in saved if item.get("path"))
    web_rule = next(item for item in saved if item.get("service", "").startswith("http://web"))
    assert api_rule["service"] == "http://api:3001"  # untouched
    assert web_rule["service"] == "http://web:3999"


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_validates_new_rule_fields(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider()
    locked = CloudflareService(provider, audit, operations_enabled=False)
    with pytest.raises(CloudflareOperationsDisabled):
        await locked.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="a.example.com", service="http://x:80")

    service = CloudflareService(provider, audit, operations_enabled=True)

    with pytest.raises(ValueError, match="invalid service target"):
        await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="a.example.com", service="not-a-service")

    with pytest.raises(ValueError):
        await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="a.example.com", service="http://x:80", connect_timeout_seconds=999)

    with pytest.raises(ValueError):
        await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="a.example.com", service="http://x:80", http_host_header="bad header!")

    # Cloudflare's browser-rendered Infrastructure Access services must be accepted, not just http(s)/tcp.
    await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="rdp.example.com", service="rdp://192.168.0.5:3389")
    saved = provider.put_ingress_calls[-1]
    assert any(item.get("service") == "rdp://192.168.0.5:3389" for item in saved)

    await service.upsert_tunnel_rule(
        original_hostname=None,
        original_path=None,
        hostname="b.example.com",
        service="http://x:80",
        path="/api/*",
        http_host_header="internal.example",
        origin_server_name="internal.example",
        connect_timeout_seconds=30,
    )
    saved = provider.put_ingress_calls[-1]
    added = next(item for item in saved if item.get("hostname") == "b.example.com")
    assert added["path"] == "/api/*"
    assert added["originRequest"]["connectTimeout"] == "30s"


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_catch_all_replaces_only_its_service(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider(ingress=[{"hostname": "a.example.com", "service": "http://x:80"}, {"service": "http_status:404"}])
    service = CloudflareService(provider, audit, operations_enabled=True)

    result = await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname=None, service="http_status:530")

    saved = provider.put_ingress_calls[0]
    assert saved[0] == {"hostname": "a.example.com", "service": "http://x:80"}
    assert saved[-1] == {"service": "http_status:530"}
    assert result is None  # no hostname to publish DNS for


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_creates_dns_record_when_none_exists(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    # "app.dreamteclabs.com" itself isn't a zone -- only "dreamteclabs.com" is,
    # so this also exercises walking down the hostname's suffixes.
    provider = FakeCloudflareProvider(known_zones={"dreamteclabs.com"}, tunnel_id="119ff1e9-abc")
    service = CloudflareService(provider, audit, operations_enabled=True)

    note = await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="app.dreamteclabs.com", service="http://192.168.0.10:80")

    assert note == "DNS record created: CNAME app.dreamteclabs.com -> 119ff1e9-abc.cfargotunnel.com."
    assert provider.created == [
        ("dreamteclabs.com", {"type": "CNAME", "name": "app.dreamteclabs.com", "content": "119ff1e9-abc.cfargotunnel.com", "ttl": 1, "proxied": True, "priority": None, "data": None})
    ]


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_leaves_existing_dns_record_alone(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider(
        records=[DnsRecord(id="r1", type="A", name="app.dreamteclabs.com", content="1.2.3.4", ttl=1, proxied=False)],
        known_zones={"dreamteclabs.com"},
    )
    service = CloudflareService(provider, audit, operations_enabled=True)

    note = await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="app.dreamteclabs.com", service="http://192.168.0.10:80")

    assert note == "DNS record already exists for this hostname -- left unchanged."
    assert provider.created == []


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_notes_when_no_zone_matches_and_still_saves_the_rule(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider(known_zones={"dreamteclabs.com"})  # "typo-domain.com" is not in this account
    service = CloudflareService(provider, audit, operations_enabled=True)

    note = await service.upsert_tunnel_rule(original_hostname=None, original_path=None, hostname="app.typo-domain.com", service="http://x:80")

    assert "no Cloudflare zone in this account matches" in note
    assert provider.created == []
    # The ingress rule itself must still have been saved despite the DNS miss.
    saved = provider.put_ingress_calls[0]
    assert any(item.get("hostname") == "app.typo-domain.com" for item in saved)


@pytest.mark.asyncio
async def test_cloudflare_service_upsert_to_blank_hostname_removes_the_original_named_rule(tmp_path: Path) -> None:
    """Clearing a named rule's hostname in the edit form routes into the
    catch-all branch (hostname is now empty) -- the original named rule must
    be removed too, not left behind alongside a freshly appended catch-all."""
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider(
        ingress=[{"hostname": "a.example.com", "service": "http://old:80"}, {"service": "http_status:404"}]
    )
    service = CloudflareService(provider, audit, operations_enabled=True)

    await service.upsert_tunnel_rule(original_hostname="a.example.com", original_path=None, hostname=None, service="http_status:530")

    saved = provider.put_ingress_calls[0]
    assert not any(item.get("hostname") == "a.example.com" for item in saved)
    assert saved == [{"service": "http_status:530"}]


@pytest.mark.asyncio
async def test_cloudflare_service_delete_tunnel_rule_removes_only_the_match(tmp_path: Path) -> None:
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    provider = FakeCloudflareProvider(
        ingress=[
            {"hostname": "family.example.com", "service": "http://api:3001", "path": "/api/v1/*"},
            {"hostname": "family.example.com", "service": "http://web:3000"},
            {"hostname": "a.example.com", "service": "http://x:80"},
            {"service": "http_status:404"},
        ]
    )
    service = CloudflareService(provider, audit, operations_enabled=True)

    await service.delete_tunnel_rule("family.example.com", "/api/v1/*")

    saved = provider.put_ingress_calls[0]
    hostnames_with_path = [(item.get("hostname"), item.get("path")) for item in saved]
    assert ("family.example.com", "/api/v1/*") not in hostnames_with_path
    assert ("family.example.com", None) in hostnames_with_path  # the other family.example.com rule survives
    assert ("a.example.com", None) in hostnames_with_path
    assert saved[-1] == {"service": "http_status:404"}
    assert audit.list_recent(1)[0].action == "tunnel-ingress-delete"


def test_cloudflare_dns_page_and_tunnel_page_render(tmp_path: Path) -> None:
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_DOMAINS_OPERATIONS_ENABLED=False)
    app = create_app(settings)
    provider = FakeCloudflareProvider(
        records=[DnsRecord(id="r1", type="A", name="example.com", content="1.2.3.4", ttl=1, proxied=True)],
        ingress=[{"hostname": "app.example.com", "service": "http://192.168.0.10:80"}, {"service": "http_status:404"}],
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


def test_cloudflare_dns_page_defaults_to_first_nexus_domain_without_zone_param(tmp_path: Path) -> None:
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_DOMAINS_OPERATIONS_ENABLED=False)
    app = create_app(settings)
    app.state.cloudflare_service = CloudflareService(
        FakeCloudflareProvider(records=[DnsRecord(id="r1", type="A", name="claudiokaist.com", content="9.9.9.9", ttl=1, proxied=False)]),
        JsonlDomainAuditRepository(tmp_path / "audit.jsonl"),
        operations_enabled=False,
    )

    with TestClient(app) as client:
        # No ?zone= at all -- should default to the first Nexus-known domain
        # (the seeded default inventory sorts "claudiokaist.com" first) rather
        # than 422ing on a missing required query param.
        page = client.get("/domains/dns")
        assert page.status_code == 200
        assert "claudiokaist.com" in page.text
        assert "9.9.9.9" in page.text
        assert 'id="zone-picker"' in page.text
        assert "dreamteclabs.com" in page.text  # another known domain listed in the picker


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

        ingress_saved = client.put(
            "/api/v1/cloudflare/tunnel/ingress",
            json={"original_hostname": None, "original_path": None, "hostname": "app.example.com", "service": "http://x:80"},
        )
        assert ingress_saved.status_code == 200
        body = ingress_saved.json()
        assert body["hostname"] == "app.example.com"
        assert body["saved"] is True
        assert body["dns_note"] == "DNS record created: CNAME app.example.com -> tunnel-abc.cfargotunnel.com."
        assert provider.put_ingress_calls[0][-1] == {"service": "http_status:404"}

        ingress_deleted = client.delete("/api/v1/cloudflare/tunnel/ingress", params={"hostname": "app.example.com"})
        assert ingress_deleted.status_code == 200
        assert ingress_deleted.json() == {"hostname": "app.example.com", "deleted": True}
        assert all(item.get("hostname") != "app.example.com" for item in provider.put_ingress_calls[-1])
