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
    data = {"kind":"lxc","remote":"homelab","node":"pve-01","vmid":150,"name":"demo-01","cores":2,"memory_mb":2048,"disk_gb":16,"storage":"local-lvm","bridge":"vmbr0","source":"local:vztmpl/debian.tar.zst","ip_config":"192.168.0.50/24","gateway":"192.168.0.1","nameserver":"192.168.0.1","onboot":True,"start":True,"unprivileged":True,"nesting":True,"ssh_enabled":True,"ssh_public_key":"ssh-ed25519 AAAATEST nexus","monitoring":"pdm","advanced":{"tags":"nexus;lab"}}
    data.update(overrides)
    return GuestProvisionRequest(**data)


@pytest.mark.asyncio
async def test_pdm_provisioning_reads_options_and_next_vmid() -> None:
    paths: list[str] = []
    def handler(request: httpx.Request) -> httpx.Response:
        paths.append(request.url.path)
        if request.url.path == "/api2/json/resources/list":
            return httpx.Response(200, json={"data":[{"remote":"homelab","resources":[{"type":"pve-node","id":"remote/homelab/node/pve-01","node":"pve-01","status":"online"},{"type":"pve-storage","id":"remote/homelab/storage/pve-01/local-lvm","storage":"local-lvm","node":"pve-01","status":"available"},{"type":"pve-network","id":"remote/homelab/network/pve-01/vmbr0","network":"vmbr0","node":"pve-01"},{"type":"lxc","id":"remote/homelab/guest/101","node":"pve-01","vmid":101},{"type":"qemu","id":"remote/homelab/guest/150","node":"pve-01","vmid":150}]}]})
        if request.url.path == "/api2/json/pve/remotes/homelab/cluster-nextid":
            return httpx.Response(200, json={"data":151})
        return httpx.Response(404)
    provider = PdmProvisioningProvider(base_url="https://pdm.test", verify_tls=False, timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    options = await provider.options()
    assert options.nodes == (ProvisioningNode("homelab","pve-01","online"),)
    assert options.storages == (ProvisioningStorage("homelab","pve-01","local-lvm","available"),)
    assert options.networks[0].name == "vmbr0"
    assert options.next_vmids == {"homelab":151}
    assert options.used_vmids == {"homelab":(101,150)}
    assert paths == ["/api2/json/resources/list","/api2/json/pve/remotes/homelab/cluster-nextid"]


@pytest.mark.asyncio
async def test_pdm_provisioning_lists_storage_content() -> None:
    captured: dict[str, object] = {}
    def handler(request: httpx.Request) -> httpx.Response:
        captured["path"] = request.url.path
        captured["query"] = dict(request.url.params)
        return httpx.Response(200, json={"data":[{"volid":"local:vztmpl/debian-13.tar.zst"},{"volid":"local:vztmpl/alpine.tar.zst"},{"volid":"local:vztmpl/debian-13.tar.zst"}]})
    provider = PdmProvisioningProvider(base_url="https://pdm.test", verify_tls=False, timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    volids = await provider.storage_content("homelab", "pve-01", "local", "vztmpl")
    assert captured["path"] == "/api2/json/pve/remotes/homelab/nodes/pve-01/storage/local/content"
    assert captured["query"] == {"content": "vztmpl"}
    assert volids == ("local:vztmpl/alpine.tar.zst", "local:vztmpl/debian-13.tar.zst")


@pytest.mark.asyncio
async def test_pdm_provisioning_storage_content_surfaces_http_errors() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(501)
    provider = PdmProvisioningProvider(base_url="https://pdm.test", verify_tls=False, timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    with pytest.raises(RuntimeError, match="HTTP 501"):
        await provider.storage_content("homelab", "pve-01", "local", "iso")


@pytest.mark.asyncio
async def test_pdm_provisioning_forwards_native_lxc_config_through_pdm_without_nexus_metadata() -> None:
    captured: dict[str, object] = {}
    def handler(request: httpx.Request) -> httpx.Response:
        captured["path"] = request.url.path
        captured["auth"] = request.headers.get("Authorization")
        captured["config"] = json.loads(json.loads(request.content)["config"])
        return httpx.Response(200, json={"data":"UPID:pve-01:0001:CREATE:150:root@pam:"})
    provider = PdmProvisioningProvider(base_url="https://pdm.test", verify_tls=False, timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    request = _request(advanced={"tags":"nexus;lab","monitoring_port":9100,"monitoring_path":"/metrics","monitoring_site":"home","health_metric":"up"})
    result = await provider.create_guest(request)
    config = captured["config"]
    assert captured["path"] == "/api2/json/pve/remotes/homelab/nodes/pve-01/create-lxc"
    assert captured["auth"] == "PDMAPIToken nexus@pam!core:secret"
    assert config["vmid"] == 150 and config["hostname"] == "demo-01" and config["rootfs"] == "local-lvm:16"
    assert config["features"] == "nesting=1" and config["ssh-public-keys"].startswith("ssh-ed25519") and config["tags"] == "nexus;lab"
    for key in ("monitoring_port","monitoring_path","monitoring_site","health_metric","monitoring_address"):
        assert key not in config
    assert result.resource_id == "remote/homelab/guest/150"


class FakeProvider:
    def __init__(self) -> None:
        self.created = False
    async def options(self) -> ProvisioningOptions:
        return ProvisioningOptions(nodes=(ProvisioningNode("homelab","pve-01","online"),), storages=(ProvisioningStorage("homelab","pve-01","local-lvm","available"),), networks=(), next_vmids={"homelab":150})
    async def create_guest(self, request: GuestProvisionRequest) -> GuestCreateResult:
        self.created = True
        return GuestCreateResult("task-1", f"remote/{request.remote}/guest/{request.vmid}")


class FakeInfrastructure:
    def __init__(self, resources=()) -> None:
        self.resources = tuple(resources)
    async def list_resources(self) -> InfrastructureSnapshot:
        return InfrastructureSnapshot(self.resources)


class AppearingInfrastructure:
    def __init__(self) -> None:
        self.calls = 0
    async def list_resources(self) -> InfrastructureSnapshot:
        self.calls += 1
        if self.calls == 1:
            return InfrastructureSnapshot(())
        return InfrastructureSnapshot((InfrastructureResource(id="remote/homelab/guest/150", provider="pdm", remote="homelab", type="pve-lxc", name="demo-01", status="running", node="pve-01", vmid=150),))


class FakeMonitoring:
    def __init__(self, fail: bool = False) -> None:
        self.calls: list[dict[str, object]] = []
        self.fail = fail
    async def upsert_target(self, **kwargs):
        self.calls.append(kwargs)
        if self.fail:
            raise OSError("simulated repository failure")
        return object()


@pytest.mark.asyncio
async def test_plan_rejects_duplicate_vmid_before_mutation() -> None:
    existing = InfrastructureResource(id="remote/homelab/guest/150", provider="pdm", remote="homelab", type="pve-lxc", name="existing", status="running", node="pve-01", vmid=150)
    provider = FakeProvider(); service = ProvisioningService(provider, FakeInfrastructure((existing,)), FakeMonitoring(), enabled=True)
    with pytest.raises(ProvisioningConflict, match="VMID 150"):
        await service.plan(_request())
    assert provider.created is False


@pytest.mark.asyncio
async def test_post_create_monitoring_failure_returns_ready_with_warnings() -> None:
    provider = FakeProvider()
    service = ProvisioningService(provider, AppearingInfrastructure(), FakeMonitoring(fail=True), enabled=True, verification_attempts=1)
    result = await service.provision(_request(monitoring="prometheus", advanced={"monitoring_port":9100,"monitoring_path":"/metrics"}))
    assert provider.created is True
    assert result.status == "ready-with-warnings"
    assert any("monitoring registration failed" in warning for warning in result.warnings)
    assert next(step for step in result.steps if step.name == "monitoring").status == "warning"


@pytest.mark.asyncio
async def test_icmp_monitoring_mode_registers_agentless_target() -> None:
    provider = FakeProvider()
    monitoring = FakeMonitoring()
    service = ProvisioningService(provider, AppearingInfrastructure(), monitoring, enabled=True, verification_attempts=1)
    result = await service.provision(_request(monitoring="icmp", advanced={"monitoring_site": "lab"}))
    assert provider.created is True
    assert result.status == "ready"
    assert monitoring.calls == [{"name": "demo-01", "address": "192.168.0.50", "profile": "icmp", "site": "lab", "state": "enabled", "health_metric": "probe_success"}]
    assert next(step for step in result.steps if step.name == "monitoring").status == "success"


@pytest.mark.asyncio
async def test_icmp_monitoring_without_static_address_warns() -> None:
    provider = FakeProvider()
    monitoring = FakeMonitoring()
    service = ProvisioningService(provider, AppearingInfrastructure(), monitoring, enabled=True, verification_attempts=1)
    result = await service.provision(_request(monitoring="icmp", ip_config="dhcp"))
    assert result.status == "ready-with-warnings"
    assert monitoring.calls == []
    assert any("ICMP monitoring requires a static IP/address" in warning for warning in result.warnings)


@pytest.mark.asyncio
async def test_root_password_too_short_is_rejected() -> None:
    service = ProvisioningService(FakeProvider(), AppearingInfrastructure(), FakeMonitoring(), enabled=True, verification_attempts=1)
    with pytest.raises(ValueError, match="at least 5 characters"):
        await service.provision(_request(root_password="ab"))


@pytest.mark.asyncio
async def test_root_password_reports_success_step_for_lxc() -> None:
    provider = FakeProvider()
    service = ProvisioningService(provider, AppearingInfrastructure(), FakeMonitoring(), enabled=True, verification_attempts=1)
    result = await service.provision(_request(root_password="correct-horse"))
    assert provider.created is True
    assert next(step for step in result.steps if step.name == "root-password").status == "success"
    assert result.status == "ready"


@pytest.mark.asyncio
async def test_root_password_warns_and_is_not_applied_for_qemu() -> None:
    provider = FakeProvider()
    service = ProvisioningService(provider, AppearingInfrastructure(), FakeMonitoring(), enabled=True, verification_attempts=1)
    result = await service.provision(_request(kind="qemu", root_password="correct-horse", source="local:iso/debian.iso"))
    assert result.status == "ready-with-warnings"
    assert any("not applied to QEMU" in warning for warning in result.warnings)
    assert next(step for step in result.steps if step.name == "root-password").status == "warning"


@pytest.mark.asyncio
async def test_provisioning_stays_locked_by_default() -> None:
    service = ProvisioningService(FakeProvider(), FakeInfrastructure(), FakeMonitoring(), enabled=False)
    with pytest.raises(ProvisioningDisabled):
        await service.provision(_request())


def test_storage_content_route_returns_volids(tmp_path: Path) -> None:
    class FakeStorageProvider(FakeProvider):
        async def storage_content(self, remote, node, storage, content):
            assert (remote, node, storage, content) == ("homelab", "pve-01", "local", "vztmpl")
            return ("local:vztmpl/alpine.tar.zst", "local:vztmpl/debian-13.tar.zst")
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_PROVISIONING_ENABLED=True))
    app.state.provisioning_service = ProvisioningService(FakeStorageProvider(), FakeInfrastructure(), FakeMonitoring(), enabled=True)
    with TestClient(app) as client:
        response = client.get("/api/v1/provisioning/storage-content", params={"remote": "homelab", "node": "pve-01", "storage": "local", "content": "vztmpl"})
        assert response.status_code == 200
        assert response.json() == ["local:vztmpl/alpine.tar.zst", "local:vztmpl/debian-13.tar.zst"]


def test_storage_content_route_surfaces_provider_errors(tmp_path: Path) -> None:
    class FailingStorageProvider(FakeProvider):
        async def storage_content(self, remote, node, storage, content):
            raise RuntimeError("PDM storage content listing returned HTTP 501")
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_PROVISIONING_ENABLED=True))
    app.state.provisioning_service = ProvisioningService(FailingStorageProvider(), FakeInfrastructure(), FakeMonitoring(), enabled=True)
    with TestClient(app) as client:
        response = client.get("/api/v1/provisioning/storage-content", params={"remote": "homelab", "node": "pve-01", "storage": "local", "content": "iso"})
        assert response.status_code == 503


def test_provisioning_ui_and_api_are_safe_when_locked(tmp_path: Path) -> None:
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_PROVISIONING_ENABLED=False))
    app.state.provisioning_service = ProvisioningService(FakeProvider(), FakeInfrastructure(), FakeMonitoring(), enabled=False)
    with TestClient(app) as client:
        page = client.get("/provisioning")
        assert page.status_code == 200 and "Provisioning Center" in page.text and "Creation locked" in page.text and "Nexus Setup" in page.text and "Prometheus / SigNoz" in page.text
        response = client.post("/api/v1/provisioning", json={"kind":"lxc","remote":"homelab","node":"pve-01","vmid":150,"name":"demo-01","cores":2,"memory_mb":2048,"disk_gb":16,"storage":"local-lvm","bridge":"vmbr0","source":"local:vztmpl/debian.tar.zst","ssh_enabled":True,"ssh_public_key":"ssh-ed25519 AAAATEST nexus","monitoring":"pdm"})
        assert response.status_code == 403
