from __future__ import annotations

import time
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.providers.pdm import PdmProvider
from nexus_core.services.backup import BackupService

LXC_FRESH = InfrastructureResource(id="remote/homelab/guest/101", provider="pdm", remote="homelab", type="pve-lxc", name="web-01", status="running", node="pve-01", vmid=101)
LXC_STALE = InfrastructureResource(id="remote/homelab/guest/102", provider="pdm", remote="homelab", type="pve-lxc", name="db-01", status="running", node="pve-01", vmid=102)
LXC_NEVER = InfrastructureResource(id="remote/homelab/guest/103", provider="pdm", remote="homelab", type="pve-lxc", name="new-01", status="running", node="pve-01", vmid=103)
DATASTORE = InfrastructureResource(id="remote/homelab/datastore/backup", provider="pdm", remote="homelab", type="pbs-datastore", name="backup", status="available")

NOW = int(time.time())


@pytest.mark.asyncio
async def test_pdm_pbs_snapshots_parses_data() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api2/json/pbs/remotes/homelab/datastore/backup/snapshots"
        return httpx.Response(200, json={"data": [{"backup-type": "ct", "backup-id": "101", "backup-time": NOW, "size": 1024, "comment": "web-01"}]})

    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    snapshots = await provider.pbs_snapshots("homelab", "backup")
    assert snapshots == [{"backup-type": "ct", "backup-id": "101", "backup-time": NOW, "size": 1024, "comment": "web-01"}]


@pytest.mark.asyncio
async def test_pdm_pbs_snapshots_returns_empty_on_http_error() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(404, text="not found")

    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    assert await provider.pbs_snapshots("homelab", "backup") == []


class FakeInfrastructure:
    def __init__(self, resources) -> None:
        self.resources = tuple(resources)

    async def list_resources(self) -> InfrastructureSnapshot:
        return InfrastructureSnapshot(self.resources)


class FakePdm:
    def __init__(self, snapshots_by_datastore: dict[tuple[str, str], list[dict] | Exception]) -> None:
        self.snapshots_by_datastore = snapshots_by_datastore
        self.calls: list[tuple[str, str]] = []

    async def pbs_snapshots(self, remote: str, datastore: str) -> list[dict]:
        self.calls.append((remote, datastore))
        result = self.snapshots_by_datastore.get((remote, datastore), [])
        if isinstance(result, Exception):
            raise result
        return result


def _snap(vmid: str, when: int, *, guest_type: str = "ct", size: int = 1024, comment: str = "") -> dict:
    return {"backup-type": guest_type, "backup-id": vmid, "backup-time": when, "size": size, "comment": comment}


@pytest.mark.asyncio
async def test_overview_marks_fresh_and_stale_guests() -> None:
    infra = FakeInfrastructure([LXC_FRESH, LXC_STALE, LXC_NEVER, DATASTORE])
    pdm = FakePdm({("homelab", "backup"): [_snap("101", NOW - 3600), _snap("102", NOW - 100 * 3600)]})
    service = BackupService(infra, pdm, stale_after_hours=48)

    overview = await service.overview()

    by_name = {g.name: g for g in overview.guests}
    assert by_name["web-01"].stale is False
    assert by_name["web-01"].last_backup.size_bytes == 1024
    assert by_name["db-01"].stale is True
    assert by_name["new-01"].stale is True
    assert by_name["new-01"].last_backup is None
    assert by_name["new-01"].backup_count == 0


@pytest.mark.asyncio
async def test_overview_counts_multiple_snapshots_and_keeps_latest() -> None:
    infra = FakeInfrastructure([LXC_FRESH, DATASTORE])
    pdm = FakePdm({("homelab", "backup"): [_snap("101", NOW - 7200), _snap("101", NOW - 100)]})
    service = BackupService(infra, pdm)

    overview = await service.overview()

    guest = overview.guests[0]
    assert guest.backup_count == 2
    assert guest.last_backup.timestamp_epoch == NOW - 100


@pytest.mark.asyncio
async def test_overview_surfaces_orphan_backups() -> None:
    infra = FakeInfrastructure([DATASTORE])  # no guests at all
    pdm = FakePdm({("homelab", "backup"): [_snap("999", NOW - 100, comment="deleted-guest")]})
    service = BackupService(infra, pdm)

    overview = await service.overview()

    assert overview.guests == ()
    assert len(overview.orphans) == 1
    assert overview.orphans[0].vmid == "999"
    assert overview.orphans[0].guest_type == "ct"


@pytest.mark.asyncio
async def test_overview_one_bad_datastore_does_not_blank_others() -> None:
    infra = FakeInfrastructure([LXC_FRESH, DATASTORE])
    pdm = FakePdm({("homelab", "backup"): RuntimeError("boom")})
    service = BackupService(infra, pdm)

    overview = await service.overview()

    assert len(overview.datastore_errors) == 1
    assert "boom" not in overview.datastore_errors[0]  # only the exception type name is surfaced
    assert overview.guests[0].last_backup is None
    assert overview.guests[0].stale is True


@pytest.mark.asyncio
async def test_overview_ignores_malformed_snapshot_entries() -> None:
    infra = FakeInfrastructure([LXC_FRESH, DATASTORE])
    pdm = FakePdm({("homelab", "backup"): [{"unexpected": "shape"}]})
    service = BackupService(infra, pdm)

    overview = await service.overview()

    assert overview.guests[0].last_backup is None
    assert overview.orphans == ()


def _app(tmp_path: Path):
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None)
    app = create_app(settings)
    infra = FakeInfrastructure([LXC_FRESH, DATASTORE])
    pdm = FakePdm({("homelab", "backup"): [_snap("101", NOW - 100)]})
    app.state.backup_service = BackupService(infra, pdm)
    return app


def test_backup_api_route_returns_overview(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/api/v1/backup")
    assert response.status_code == 200
    payload = response.json()
    assert payload["guests"][0]["name"] == "web-01"
    assert payload["guests"][0]["stale"] is False


def test_backup_page_renders(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/backup")
    assert response.status_code == 200
    assert "Backup" in response.text
    assert "web-01" in response.text
