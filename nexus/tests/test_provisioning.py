from __future__ import annotations

import json
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.ports.provisioning import GuestCreateResult, GuestProvisionRequest, ProvisioningNode, ProvisioningOptions, ProvisioningStorage
from nexus_core.providers.pdm_provisioning import PdmProvisioningProvider
from nexus_core.services.provisioning import ProvisioningConflict, ProvisioningDisabled, ProvisioningService


def _request(**overrides) -> GuestProvisionRequest:
    data = {
        "kind": "lxc",
        "remote": "homelab",
        "node": "pve-01",
        "vmid": 150,
        "name": "demo-01",
        "cores": 2,
        "memory_mb": 2048,
        "disk_gb": 16,
        "storage": "local-lvm",
        "bridge": "vmbr0",
        "source": "local:vztmpl/debian.tar.zst",
        "ip_config": "192.168.0.50/24",
        "gateway": "192.168.0.1",
        "nameserver": "192.168.0.1",
        "onboot": True,
        "start": True,
        "unprivileged": True,
        "nesting": True,
        "ssh_enabled": True,
        "ssh_public_key": "ssh-ed25519 AAAATEST nexus",
        "monitoring": "pdm",
        "advanced": {"tags": "nexus;lab"},
    }
    data.update(overrides)
    return GuestProvisionRequest(**data)


@pytest.mark.asyncio
async def test_pdm_provisioning_reads_options_and_next_vmid() -> None:
    paths: list[str] = []

    def handler(request: httpx.Request) -> httpx.Response:
        paths.append(request.url.path)
        if request.url.path == "/api2/json/resources/list":
            return httpx.Response(200, json={"data": [{"remote": "homelab", "resources": [
                {"type": "pve-node", "id": "remote/homelab/node/pve-01", "node": "pve-01", "status": "online"},
                {"type": "pve-storage", "id": "remote/homelab/storage/pve-01/local-lvm", "storage": "local-lvm", "node": "pve-01", "status": "available"},
                {"type": "pve-network", "id": "remote/homelab/network/pve-01/vmbr0", "network": "vmbr0", "node": "pve-01"},
            ]}]})
        if request.url.path == "/api2/json/pve/remotes/homelab/cluster-nextid":
            return httpx.Response(200, json={"data": 151})
        return httpx.Response(404)

    provider = PdmProvisioningProvider(base_url="https://pdm.test", verify_tls=False, timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    options = await provider.options()
    assert options.nodes == (ProvisioningNode("homelab", "pve-01", "online"),)
    assert options.storages == (ProvisioningStorage("homelab", "pve-01", "local-lvm", "available"),)
    assert options.networks[0].name == "vmbr0"
    assert options.next_vmids == {"homelab": 151}
    assert paths == ["/api2/json/resources/list", "/api2/json/pve/remotes/homelab/cluster-nextid"]


@pytest.mark.asyncio
async def test_pdm_provisioning_forwards_native_lxc_config_through_pdm() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["path"] = request.url.path
        captured["auth"] = request.headers.get("Authorization")
        body = json.loads(request.content)
        captured["config"] = json.loads(body["config"])
        return httpx.Response(200, json={"data": "UPID:pve-01:0001:CREATE:150:root@pam:"})

    provider = PdmProvisioningProvider(base_url="https://pdm.test", verify_tls=False, timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    result = await provider.create_guest(_request())
    assert captured["path"] == "/api2/json/pve/remotes/homelab/nodes/pve-01/create-lxc"
    assert captured["auth"] == "PDMAPIToken nexus@pam!core:secret"
    config = captured["config"]
    assert config["vmid"] == 150
    assert config["hostname"] == "demo-01"
    assert config["rootfs"] == "local-lvm:16"
    assert config["features"] == "nesting=1"
    assert config["ssh-public-keys"].startswith("ssh-ed25519")
    assert config["tags"] == "nexus;lab"
    assert result.resource_id == "remote/homelab/guest/150"


class FakeProvider:
    def __init__(self) -> None:
        self.created = False

    async def options(self) -> ProvisioningOptions:
        return ProvisioningOptions(nodes=(ProvisioningNode("homelab", "pve-01", "online"),), storages=(ProvisioningStorage("homelab", "pve-01", "local-lvm", "available"),), networks=(), next_vmids={"homelab": 150})

    async def create_guest(self, request: GuestProvisionRequest) -> GuestCreateResult:
        self.created = True
        return GuestCreateResult("task-1", f"remote/{request.remote}/guest/{request.vmid}")


class FakeInfrastructure:
    def __init__(self, resources=()) -> None:
        self.resources = tuple(resources)

    async def list_resources(self) -> InfrastructureSnapshot:
        return InfrastructureSnapshot(self.resources)


class FakeMonitoring:
    def __init__(self) -> None:
        self.calls: list[dict[str, object]] = []

    async def upsert_target(self, **kwargs):
        self.calls.append(kwargs)
        return object()


@pytest.mark.asyncio
async def test_plan_rejects_duplicate_vmid_before_mutation() -> None:
    existing = InfrastructureResource(id="remote/homelab/guest/150", provider="pdm", remote="homelab", type="pve-lxc", name="existing", status="running", node="pve-01", vmid=150)
    provider = FakeProvider()
    service = ProvisioningService(provider, FakeInfrastructure((existing,)), FakeMonitoring(), enabled=True)
    with pytest.raises(ProvisioningConflict, match="VMID 150"):
        await service.plan(_request())
    assert provider.created is False


@pytest.mark.asyncio
async def test_provisioning_stays_locked_by_default() -> None:
    service = ProvisioningService(FakeProvider(), FakeInfrastructure(), FakeMonitoring(), enabled=False)
    with pytest.raises(ProvisioningDisabled):
        await service.provision(_request())


def test_provisioning_ui_and_api_are_safe_when_locked(tmp_path: Path) -> None:
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_PROVISIONING_ENABLED=False))
    fake = FakeProvider()
    app.state.provisioning_service = ProvisioningService(fake, FakeInfrastructure(), FakeMonitoring(), enabled=False)
    with TestClient(app) as client:
        page = client.get("/provisioning")
        assert page.status_code == 200
        assert "Provisioning Center" in page.text
        assert "Creation locked" in page.text
        assert "Nexus Setup" in page.text
        assert "Prometheus / SigNoz" in page.text
        response = client.post("/api/v1/provisioning", json={
            "kind":"lxc","remote":"homelab","node":"pve-01","vmid":150,"name":"demo-01","cores":2,"memory_mb":2048,"disk_gb":16,"storage":"local-lvm","bridge":"vmbr0","source":"local:vztmpl/debian.tar.zst","ssh_enabled":True,"ssh_public_key":"ssh-ed25519 AAAATEST nexus","monitoring":"pdm"
        })
        assert response.status_code == 403
