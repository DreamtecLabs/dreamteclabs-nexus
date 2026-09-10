from __future__ import annotations

from pathlib import Path

from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.domains import DomainCheck, DomainOperationResult, DomainRecord, DomainValidation
from nexus_core.providers.domain_diagnostics import PublicDomainDiagnosticsProvider
from nexus_core.repositories.domain_audit_jsonl import JsonlDomainAuditRepository
from nexus_core.repositories.domains_json import JsonDomainRepository
from nexus_core.services.domains import DomainOperationsDisabled, DomainService


class FakeDiagnostics:
    def __init__(self, healthy: bool = True) -> None:
        self.healthy = healthy

    async def validate(self, domain: DomainRecord) -> DomainValidation:
        return DomainValidation(
            domain.name,
            self.healthy,
            (DomainCheck("mx", "MX", self.healthy, "mail.example.com"),),
        )


class FakeOrchestrator:
    async def reconcile(self, domain: DomainRecord, *, migrate: bool) -> DomainOperationResult:
        return DomainOperationResult(
            domain.name,
            "migrate" if migrate else "onboard",
            True,
            ("cloudflare:dns", "hestia:mail"),
        )


async def _no_sleep(_: float) -> None:
    return None


async def test_tls_check_drains_the_server_greeting_before_closing(monkeypatch) -> None:
    # Mail protocols push a greeting the instant the TLS handshake completes (e.g. IMAP's
    # "* OK ... ready"). Closing before reading it races our close_notify against that data
    # and OpenSSL raises APPLICATION_DATA_AFTER_CLOSE_NOTIFY even though the handshake and
    # certificate were fine — this reproduces that ordering without a real TLS socket.
    calls: list[str] = []

    class FakeReader:
        async def read(self, _n: int) -> bytes:
            calls.append("read")
            return b"* OK IMAP4rev1 ready\r\n"

    class FakeWriter:
        def close(self) -> None:
            calls.append("close")

        async def wait_closed(self) -> None:
            calls.append("wait_closed")

    async def fake_open_connection(_host, _port, *, ssl=None, server_hostname=None):
        return FakeReader(), FakeWriter()

    monkeypatch.setattr("nexus_core.providers.domain_diagnostics.asyncio.open_connection", fake_open_connection)

    provider = PublicDomainDiagnosticsProvider()
    ok, detail = await provider._tcp("mail.example.com", 993, tls=True)
    assert ok is True
    assert detail == "mail.example.com:993 reachable"
    assert calls == ["read", "close", "wait_closed"]


async def test_non_tls_check_does_not_attempt_to_drain(monkeypatch) -> None:
    calls: list[str] = []

    class FakeReader:
        async def read(self, _n: int) -> bytes:
            calls.append("read")
            return b""

    class FakeWriter:
        def close(self) -> None:
            calls.append("close")

        async def wait_closed(self) -> None:
            calls.append("wait_closed")

    async def fake_open_connection(_host, _port, *, ssl=None, server_hostname=None):
        return FakeReader(), FakeWriter()

    monkeypatch.setattr("nexus_core.providers.domain_diagnostics.asyncio.open_connection", fake_open_connection)

    provider = PublicDomainDiagnosticsProvider()
    ok, _detail = await provider._tcp("mail.example.com", 587, tls=False)
    assert ok is True
    assert calls == ["close", "wait_closed"]


def test_domain_repository_seeds_known_estate_without_overwriting(tmp_path: Path) -> None:
    repo = JsonDomainRepository(tmp_path / "domains.json")
    assert len(repo.list_domains()) == 7
    repo.upsert_domain(DomainRecord("example.com", configuration_mode="discovered"))
    repo2 = JsonDomainRepository(tmp_path / "domains.json")
    assert repo2.get_domain("example.com") is not None
    assert len(repo2.list_domains()) == 8


async def test_domain_validation_does_not_mutate_inventory(tmp_path: Path) -> None:
    repo = JsonDomainRepository(tmp_path / "domains.json")
    service = DomainService(
        repo,
        FakeDiagnostics(),
        FakeOrchestrator(),
        JsonlDomainAuditRepository(tmp_path / "audit.jsonl"),
        operations_enabled=False,
    )
    assert len(repo.list_domains()) == 7
    validation = await service.validate("example.com")
    assert validation.domain == "example.com"
    assert validation.healthy is True
    assert repo.get_domain("example.com") is None
    assert len(repo.list_domains()) == 7


async def test_domain_service_requires_gate_and_verifies_mutation(tmp_path: Path) -> None:
    repo = JsonDomainRepository(tmp_path / "domains.json")
    audit = JsonlDomainAuditRepository(tmp_path / "audit.jsonl")
    locked = DomainService(repo, FakeDiagnostics(), FakeOrchestrator(), audit, operations_enabled=False)
    try:
        await locked.reconcile(name="example.com")
        raise AssertionError("expected disabled operations")
    except DomainOperationsDisabled:
        pass

    service = DomainService(
        repo,
        FakeDiagnostics(),
        FakeOrchestrator(),
        audit,
        operations_enabled=True,
        verification_interval_seconds=0,
        sleep=_no_sleep,
    )
    result, validation = await service.reconcile(name="example.com", hestia_user="admin")
    assert result.ok is True
    assert validation.healthy is True
    saved = repo.get_domain("example.com")
    assert saved is not None
    assert saved.configuration_mode == "managed"
    assert saved.mail is True
    assert audit.list_recent(1)[0].result == "success"


def test_domains_api_and_light_ui(tmp_path: Path) -> None:
    settings = Settings(
        NEXUS_DATA_DIR=tmp_path,
        PDM_BASE_URL="https://pdm.invalid",
        PDM_VERIFY_TLS=False,
        NEXUS_DOMAINS_OPERATIONS_ENABLED=False,
    )
    app = create_app(settings)
    app.state.domain_service = DomainService(
        JsonDomainRepository(tmp_path / "domains.json"),
        FakeDiagnostics(),
        FakeOrchestrator(),
        JsonlDomainAuditRepository(tmp_path / "audit.jsonl"),
        operations_enabled=False,
    )
    with TestClient(app) as client:
        listing = client.get("/api/v1/domains")
        assert listing.status_code == 200
        assert listing.json()["count"] == 7

        validation = client.post("/api/v1/domains/validate", json={"domain": "mundoleo.co"})
        assert validation.status_code == 200
        assert validation.json()["healthy"] is True

        page = client.get("/domains")
        assert page.status_code == 200
        assert "Domains & Hosting" in page.text
        assert "mundoleo.co" in page.text

        css = client.get("/static/nexus.css").text
        # Light is the unconditional :root default; dark is opt-in only, never forced.
        assert "--bg:#F3F4F9" in css
        assert "prefers-color-scheme: dark" in css

        reconcile = client.post(
            "/api/v1/domains/reconcile",
            json={"domain": "example.com", "hestia_user": "admin", "migrate": False},
        )
        assert reconcile.status_code == 403
