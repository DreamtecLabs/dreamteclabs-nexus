from __future__ import annotations

from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureRemoteError, InfrastructureResource, InfrastructureSnapshot
from nexus_core.providers.pdm import PdmProvider


@pytest.mark.asyncio
async def test_pdm_lists_and_normalizes_resources_with_token_auth() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["path"] = request.url.path
        captured["authorization"] = request.headers.get("Authorization")
        return httpx.Response(200, json={"data": [
            {"remote": "home-pve", "resources": [
                {"type": "node", "id": "remote/home-pve/node/pve-01", "node": "pve-01", "status": "online"},
                {"type": "qemu", "id": "remote/home-pve/guest/101", "name": "postgres-01", "node": "pve-01", "status": "running", "vmid": 101, "template": False},
                {"type": "lxc", "id": "remote/home-pve/guest/104", "name": "nginx", "node": "pve-01", "status": "stopped", "vmid": 104, "template": False},
                {"type": "storage", "id": "remote/home-pve/storage/pve-01/local-lvm", "storage": "local-lvm", "node": "pve-01", "status": "available"},
            ]},
            {"remote": "pbs-01", "resources": [
                {"type": "node", "id": "remote/pbs-01/node/localhost", "name": "localhost", "status": "online"},
                {"type": "datastore", "id": "remote/pbs-01/datastore/backup", "name": "backup", "status": "available"},
            ]},
            {"remote": "offline-pve", "resources": [], "error": "remote is unreachable"},
        ]})

    provider = PdmProvider(
        base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5,
        api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler),
    )
    snapshot = await provider.list_resources()

    assert captured == {"path": "/api2/json/resources/list", "authorization": "PDMAPIToken nexus@pam!core:secret"}
    assert len(snapshot.resources) == 6
    by_id = {resource.id: resource for resource in snapshot.resources}
    assert by_id["remote/home-pve/guest/104"] == InfrastructureResource(
        id="remote/home-pve/guest/104", provider="pdm", remote="home-pve", type="pve-lxc",
        name="nginx", status="stopped", node="pve-01", vmid=104, template=False,
    )
    assert by_id["remote/home-pve/guest/101"].type == "pve-qemu"
    assert by_id["remote/home-pve/node/pve-01"].type == "pve-node"
    assert by_id["remote/home-pve/storage/pve-01/local-lvm"].type == "pve-storage"
    assert by_id["remote/pbs-01/node/localhost"].type == "pbs-node"
    assert by_id["remote/pbs-01/datastore/backup"].type == "pbs-datastore"
    assert snapshot.remote_errors == (InfrastructureRemoteError(remote="offline-pve", detail="remote is unreachable"),)


@pytest.mark.asyncio
async def test_pdm_keeps_already_canonical_types_and_derives_display_names() -> None:
    def handler(_: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"data": [{"remote": "home-pve", "resources": [
            {"type": "pve-network", "id": "remote/home-pve/network/pve-01/zone/localnetwork", "status": "available", "node": "pve-01"},
            {"type": "pve-storage", "id": "remote/home-pve/storage/pve-01/local-zfs", "status": "available", "node": "pve-01"},
        ]}]})

    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    snapshot = await provider.list_resources()
    by_type = {resource.type: resource for resource in snapshot.resources}
    assert by_type["pve-network"].name == "localnetwork"
    assert by_type["pve-storage"].name == "local-zfs"


@pytest.mark.asyncio
async def test_pdm_resources_http_error_is_safe() -> None:
    def handler(_: httpx.Request) -> httpx.Response:
        return httpx.Response(403, text="secret backend details")
    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    with pytest.raises(RuntimeError, match="PDM resources API returned HTTP 403") as exc_info:
        await provider.list_resources()
    assert "secret backend details" not in str(exc_info.value)


@pytest.mark.asyncio
async def test_pdm_rejects_unsupported_resources_payload() -> None:
    def handler(_: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"data": {"unexpected": True}})
    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    with pytest.raises(RuntimeError, match="unsupported payload"):
        await provider.list_resources()


class FakeInfrastructureService:
    power_operations_enabled = False

    async def list_resources(self) -> InfrastructureSnapshot:
        return InfrastructureSnapshot(
            resources=(
                InfrastructureResource(id="remote/home-pve/guest/101", provider="pdm", remote="home-pve", type="pve-qemu", name="postgres-01", status="running", node="pve-01", vmid=101, template=False),
                InfrastructureResource(id="remote/home-pve/guest/104", provider="pdm", remote="home-pve", type="pve-lxc", name="nginx", status="stopped", node="pve-01", vmid=104, template=False),
                InfrastructureResource(id="remote/home-pve/node/pve-01", provider="pdm", remote="home-pve", type="pve-node", name="pve-01", status="online", node="pve-01"),
            ),
            remote_errors=(InfrastructureRemoteError(remote="lab-pve", detail="temporarily unavailable"),),
        )

    def available_power_actions(self, resource: InfrastructureResource) -> dict[str, bool]:
        return {"start": resource.status == "stopped", "shutdown": resource.status == "running", "stop": resource.status == "running"}


class FailingInfrastructureService:
    power_operations_enabled = False

    async def list_resources(self) -> InfrastructureSnapshot:
        raise RuntimeError("PDM resources API failed: ConnectError")


def _test_app(tmp_path: Path):
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None)
    app = create_app(settings)
    app.state.infrastructure_service = FakeInfrastructureService()
    return app


def test_infrastructure_api_returns_normalized_snapshot(tmp_path: Path) -> None:
    app = _test_app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/api/v1/infrastructure/resources")
    assert response.status_code == 200
    payload = response.json()
    assert payload["count"] == 3
    assert payload["resources"][0]["name"] == "postgres-01"
    assert payload["remote_errors"] == [{"remote": "lab-pve", "detail": "temporarily unavailable"}]
    assert payload["power_operations_enabled"] is False


def test_infrastructure_page_renders_inventory_summary_and_resources(tmp_path: Path) -> None:
    app = _test_app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/infrastructure")
    assert response.status_code == 200
    assert "Inventory" in response.text
    assert "postgres-01" in response.text
    assert "nginx" in response.text
    assert "1 running · 1 stopped" in response.text
    assert "Provider warnings" in response.text
    assert "temporarily unavailable" in response.text
    assert "Open Power Center" in response.text
    assert "inventory-search" in response.text


def test_infrastructure_page_returns_503_when_provider_fails(tmp_path: Path) -> None:
    app = _test_app(tmp_path)
    app.state.infrastructure_service = FailingInfrastructureService()
    with TestClient(app) as client:
        response = client.get("/infrastructure")
    assert response.status_code == 503
    assert response.json() == {"detail": "PDM resources API failed: ConnectError"}
