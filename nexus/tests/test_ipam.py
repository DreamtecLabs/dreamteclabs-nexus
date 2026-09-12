from __future__ import annotations

from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.providers.pdm import PdmProvider
from nexus_core.repositories.ipam_json import JsonIpamRepository
from nexus_core.services.ipam import IpamService

LXC_STATIC = InfrastructureResource(id="remote/homelab/guest/104", provider="pdm", remote="homelab", type="pve-lxc", name="nginx", status="running", node="pve-01", vmid=104, template=False)
LXC_DHCP = InfrastructureResource(id="remote/homelab/guest/105", provider="pdm", remote="homelab", type="pve-lxc", name="dhcp-guest", status="running", node="pve-01", vmid=105, template=False)
QEMU_GUEST = InfrastructureResource(id="remote/homelab/guest/106", provider="pdm", remote="homelab", type="pve-qemu", name="windows", status="running", node="pve-01", vmid=106, template=False)


@pytest.mark.asyncio
async def test_pdm_guest_static_address_parses_net0_ip() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api2/json/pve/remotes/homelab/lxc/104/config"
        return httpx.Response(200, json={"data": {"net0": "name=eth0,bridge=vmbr0,ip=192.168.0.10/24,gw=192.168.0.1", "hostname": "nginx"}})

    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    address = await provider.guest_static_address(LXC_STATIC)
    assert address == "192.168.0.10"


@pytest.mark.asyncio
async def test_pdm_guest_static_address_ignores_dhcp_net0() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"data": {"net0": "name=eth0,bridge=vmbr0,ip=dhcp"}})

    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, transport=httpx.MockTransport(handler))
    address = await provider.guest_static_address(LXC_DHCP)
    assert address is None


@pytest.mark.asyncio
async def test_pdm_guest_static_address_returns_none_for_qemu() -> None:
    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5)
    address = await provider.guest_static_address(QEMU_GUEST)
    assert address is None


class FakeInfrastructure:
    def __init__(self, resources) -> None:
        self.resources = tuple(resources)

    async def list_resources(self) -> InfrastructureSnapshot:
        return InfrastructureSnapshot(self.resources)


class FakeGuestAddresses:
    def __init__(self, mapping: dict[str, str]) -> None:
        self.mapping = mapping

    async def guest_static_address(self, resource: InfrastructureResource) -> str | None:
        return self.mapping.get(resource.id)


@pytest.mark.asyncio
async def test_snapshot_reports_guest_and_manual_entries(tmp_path: Path) -> None:
    repository = JsonIpamRepository(tmp_path / "ipam.json")
    infra = FakeInfrastructure([LXC_STATIC])
    addresses = FakeGuestAddresses({LXC_STATIC.id: "192.168.0.10"})
    service = IpamService(repository, infra, addresses)
    await service.add_manual_entry(address="192.168.0.1", label="Router")

    snapshot = await service.snapshot()

    used_by_address = {entry.address: entry for entry in snapshot.used}
    assert used_by_address["192.168.0.10"].source == "guest"
    assert used_by_address["192.168.0.10"].label == "nginx"
    assert used_by_address["192.168.0.1"].source == "manual"
    assert used_by_address["192.168.0.1"].label == "Router"
    assert "192.168.0.10" not in snapshot.free
    assert "192.168.0.1" not in snapshot.free
    assert "192.168.0.2" in snapshot.free


@pytest.mark.asyncio
async def test_snapshot_excludes_dhcp_range_from_free_addresses(tmp_path: Path) -> None:
    repository = JsonIpamRepository(tmp_path / "ipam.json")
    service = IpamService(repository, FakeInfrastructure([]), FakeGuestAddresses({}))
    snapshot = await service.snapshot()
    assert "192.168.0.100" not in snapshot.free
    assert "192.168.0.50" not in snapshot.free
    assert "192.168.0.199" not in snapshot.free
    assert "192.168.0.200" in snapshot.free
    assert "192.168.0.254" in snapshot.free
    assert "192.168.0.0" not in snapshot.free
    assert "192.168.0.255" not in snapshot.free


@pytest.mark.asyncio
async def test_manual_entry_rejected_inside_dhcp_range(tmp_path: Path) -> None:
    repository = JsonIpamRepository(tmp_path / "ipam.json")
    service = IpamService(repository, FakeInfrastructure([]), FakeGuestAddresses({}))
    with pytest.raises(ValueError, match="DHCP range"):
        await service.add_manual_entry(address="192.168.0.100", label="Something")


@pytest.mark.asyncio
async def test_manual_entry_rejected_when_guest_already_holds_address(tmp_path: Path) -> None:
    repository = JsonIpamRepository(tmp_path / "ipam.json")
    infra = FakeInfrastructure([LXC_STATIC])
    addresses = FakeGuestAddresses({LXC_STATIC.id: "192.168.0.10"})
    service = IpamService(repository, infra, addresses)
    with pytest.raises(ValueError, match="already in use by guest 'nginx'"):
        await service.add_manual_entry(address="192.168.0.10", label="Whoops")


@pytest.mark.asyncio
async def test_snapshot_surfaces_conflict_when_guest_claims_manual_address(tmp_path: Path) -> None:
    repository = JsonIpamRepository(tmp_path / "ipam.json")
    from nexus_core.ports.ipam import IpamEntry
    repository.upsert_entry(IpamEntry(address="192.168.0.10", source="manual", label="Pre-existing note"))
    infra = FakeInfrastructure([LXC_STATIC])
    addresses = FakeGuestAddresses({LXC_STATIC.id: "192.168.0.10"})
    service = IpamService(repository, infra, addresses)

    snapshot = await service.snapshot()

    assert len(snapshot.conflicts) == 1
    conflict = snapshot.conflicts[0]
    assert conflict.address == "192.168.0.10"
    assert conflict.guest_entry.label == "nginx"
    assert conflict.manual_entry.label == "Pre-existing note"
    # the guest wins over the stale manual entry in the "used" view
    assert next(entry for entry in snapshot.used if entry.address == "192.168.0.10").source == "guest"


@pytest.mark.asyncio
async def test_delete_manual_entry_rejects_guest_backed_address(tmp_path: Path) -> None:
    repository = JsonIpamRepository(tmp_path / "ipam.json")
    from nexus_core.ports.ipam import IpamEntry
    repository.upsert_entry(IpamEntry(address="192.168.0.10", source="guest", label="nginx", resource_id=LXC_STATIC.id))
    service = IpamService(repository, FakeInfrastructure([]), FakeGuestAddresses({}))
    with pytest.raises(ValueError, match="not a manually registered entry"):
        service.delete_manual_entry("192.168.0.10")


def _app(tmp_path: Path):
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None)
    app = create_app(settings)
    infra = FakeInfrastructure([LXC_STATIC])
    addresses = FakeGuestAddresses({LXC_STATIC.id: "192.168.0.10"})
    app.state.ipam_service = IpamService(JsonIpamRepository(tmp_path / "ipam.json"), infra, addresses)
    return app


def test_ipam_api_returns_snapshot(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/api/v1/ipam")
    assert response.status_code == 200
    payload = response.json()
    assert payload["cidr"] == "192.168.0.0/24"
    assert any(entry["address"] == "192.168.0.10" for entry in payload["used"])


def test_ipam_api_add_and_delete_manual_entry(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        add_response = client.post("/api/v1/ipam/entries", json={"address": "192.168.0.1", "label": "Router"})
        assert add_response.status_code == 200
        list_response = client.get("/api/v1/ipam")
        assert any(entry["address"] == "192.168.0.1" for entry in list_response.json()["used"])
        delete_response = client.delete("/api/v1/ipam/entries/192.168.0.1")
        assert delete_response.status_code == 200
        list_response = client.get("/api/v1/ipam")
        assert not any(entry["address"] == "192.168.0.1" for entry in list_response.json()["used"])


def test_ipam_api_rejects_dhcp_range_address(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.post("/api/v1/ipam/entries", json={"address": "192.168.0.100", "label": "Nope"})
    assert response.status_code == 422


def test_ipam_page_renders(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/ipam")
    assert response.status_code == 200
    assert "IP Address Management" in response.text
    assert "192.168.0.10" in response.text
