from __future__ import annotations

from pathlib import Path

from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.providers.pdm import PdmProvider
from nexus_core.services.infrastructure import InfrastructureService


class FakeProvider:
    def __init__(self, snapshot: InfrastructureSnapshot) -> None:
        self.snapshot = snapshot

    async def list_resources(self) -> InfrastructureSnapshot:
        return self.snapshot

    async def execute_power_action(self, resource: InfrastructureResource, action: str) -> str | None:
        raise AssertionError("not used")


def _snapshot() -> InfrastructureSnapshot:
    return InfrastructureSnapshot(
        resources=(
            InfrastructureResource(id="remote/home/node/pve-01", provider="pdm", remote="home", type="pve-node", name="pve-01", status="online", node="pve-01", cpu_usage=0.25, cpu_total=8, memory_used_bytes=8 * 1024**3, memory_total_bytes=32 * 1024**3, uptime_seconds=86461),
            InfrastructureResource(id="remote/home/guest/101", provider="pdm", remote="home", type="pve-qemu", name="postgres-01", status="running", node="pve-01", vmid=101, template=False, cpu_usage=0.5, cpu_total=4, memory_used_bytes=2 * 1024**3, memory_total_bytes=4 * 1024**3, disk_used_bytes=10 * 1024**3, disk_total_bytes=20 * 1024**3, uptime_seconds=3600),
            InfrastructureResource(id="remote/home/guest/104", provider="pdm", remote="home", type="pve-lxc", name="nginx", status="stopped", node="pve-01", vmid=104, template=False),
            InfrastructureResource(id="remote/home/storage/pve-01/local", provider="pdm", remote="home", type="pve-storage", name="local", status="available", node="pve-01"),
            InfrastructureResource(id="remote/home/network/pve-01/vmbr0", provider="pdm", remote="home", type="pve-network", name="vmbr0", status="available", node="pve-01"),
            InfrastructureResource(id="remote/pbs/node", provider="pdm", remote="pbs", type="pbs-node", name="pbs-01", status="online"),
            InfrastructureResource(id="remote/pbs/datastore/backup", provider="pdm", remote="pbs", type="pbs-datastore", name="backup", status="available", disk_used_bytes=100, disk_total_bytes=1000),
        )
    )


def test_pdm_normalizes_optional_resource_telemetry() -> None:
    snapshot = PdmProvider._normalize_resources([
        {"remote": "home", "resources": [{"type": "qemu", "id": "remote/home/guest/101", "name": "db", "node": "pve-01", "status": "running", "vmid": 101, "cpu": 0.125, "maxcpu": 4, "mem": 1024, "maxmem": 4096, "disk": 2048, "maxdisk": 8192, "uptime": 1234}]}
    ])
    item = snapshot.resources[0]
    assert item.cpu_usage == 0.125
    assert item.cpu_total == 4.0
    assert item.memory_used_bytes == 1024
    assert item.memory_total_bytes == 4096
    assert item.disk_used_bytes == 2048
    assert item.disk_total_bytes == 8192
    assert item.uptime_seconds == 1234


def test_pdm_ignores_invalid_optional_resource_telemetry() -> None:
    snapshot = PdmProvider._normalize_resources([
        {"remote": "home", "resources": [{"type": "lxc", "id": "x", "name": "x", "status": "stopped", "vmid": 1, "cpu": "bad", "mem": -1, "maxmem": None}]}
    ])
    item = snapshot.resources[0]
    assert item.cpu_usage is None
    assert item.memory_used_bytes is None
    assert item.memory_total_bytes is None


def test_resource_context_uses_stable_remote_and_node_relationships() -> None:
    import asyncio
    service = InfrastructureService(FakeProvider(_snapshot()))
    context = asyncio.run(service.get_resource_context("remote/home/guest/101"))
    assert context.resource.name == "postgres-01"
    assert context.node is not None and context.node.name == "pve-01"
    assert {item.name for item in context.guests} == {"nginx"}
    assert {item.name for item in context.storages} == {"local"}
    assert {item.name for item in context.networks} == {"vmbr0"}


def test_estate_summary_groups_pve_and_pbs_remotes() -> None:
    import asyncio
    service = InfrastructureService(FakeProvider(_snapshot()))
    remotes = asyncio.run(service.estate_summary())
    assert [item.remote for item in remotes] == ["home", "pbs"]
    home = remotes[0]
    assert home.guests == 2
    assert home.running == 1
    assert home.stopped == 1
    assert home.pve_nodes == 1
    assert home.nodes[0].memory_total_bytes == 32 * 1024**3
    assert remotes[1].pbs_resources == 2


def _app(tmp_path: Path):
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None))
    app.state.infrastructure_service = InfrastructureService(FakeProvider(_snapshot()))
    return app


def test_resource_detail_and_estate_api(tmp_path: Path) -> None:
    with TestClient(_app(tmp_path)) as client:
        detail = client.get("/api/v1/infrastructure/resources/detail/remote/home/guest/101")
        estate = client.get("/api/v1/infrastructure/estate")
    assert detail.status_code == 200
    assert detail.json()["resource"]["name"] == "postgres-01"
    assert detail.json()["node"]["name"] == "pve-01"
    assert estate.status_code == 200
    assert estate.json()["count"] == 2


def test_resource_detail_api_returns_404(tmp_path: Path) -> None:
    with TestClient(_app(tmp_path)) as client:
        response = client.get("/api/v1/infrastructure/resources/detail/not-found")
    assert response.status_code == 404


def test_resource_center_ui_links_inventory_detail_and_estate(tmp_path: Path) -> None:
    with TestClient(_app(tmp_path)) as client:
        inventory = client.get("/infrastructure")
        detail = client.get("/infrastructure/resource", params={"id": "remote/home/guest/101"})
        estate = client.get("/infrastructure/estate")
    assert inventory.status_code == 200
    assert "Open Estate" in inventory.text
    assert "/infrastructure/resource?id=" in inventory.text
    assert detail.status_code == 200
    assert "postgres-01" in detail.text
    assert "50.0%" in detail.text
    assert "pve-01" in detail.text
    assert "nginx" in detail.text
    assert estate.status_code == 200
    assert "Operational topology" in estate.text
    assert "32.0 GiB" in estate.text


def test_resource_center_renders_missing_metrics_without_failure(tmp_path: Path) -> None:
    with TestClient(_app(tmp_path)) as client:
        response = client.get("/infrastructure/resource", params={"id": "remote/home/guest/104"})
    assert response.status_code == 200
    assert "nginx" in response.text
    assert "—" in response.text
