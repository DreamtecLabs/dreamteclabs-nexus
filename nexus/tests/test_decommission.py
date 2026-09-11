from __future__ import annotations

import dataclasses
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.providers.pdm import PdmProvider
from nexus_core.services.infrastructure import (
    DecommissionDisabled,
    DecommissionNotAllowed,
    DecommissionVerificationTimeout,
    InfrastructureResourceNotFound,
    InfrastructureService,
)


RUNNING_LXC = InfrastructureResource(
    id="remote/home-pve/guest/104",
    provider="pdm",
    remote="home-pve",
    type="pve-lxc",
    name="nginx",
    status="running",
    node="pve-01",
    vmid=104,
    template=False,
)
STOPPED_QEMU = InfrastructureResource(
    id="remote/home-pve/guest/101",
    provider="pdm",
    remote="home-pve",
    type="pve-qemu",
    name="postgres-01",
    status="stopped",
    node="pve-02",
    vmid=101,
    template=False,
)
NODE_RESOURCE = InfrastructureResource(id="remote/home-pve/node/pve-01", provider="pdm", remote="home-pve", type="pve-node", name="pve-01", status="online")


@pytest.mark.asyncio
async def test_pdm_destroy_guest_route_method_and_params() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["method"] = request.method
        captured["path"] = request.url.path
        captured["query"] = dict(request.url.params)
        captured["authorization"] = request.headers.get("Authorization")
        return httpx.Response(200, json={"data": "UPID:home-pve:destroy"})

    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5, api_token_id="nexus@pam!core", api_token_secret="secret", transport=httpx.MockTransport(handler))
    task = await provider.destroy_guest(RUNNING_LXC, purge=True)

    assert captured["method"] == "DELETE"
    assert captured["path"] == "/api2/json/pve/remotes/home-pve/lxc/104"
    assert captured["query"] == {"purge": "1", "node": "pve-01"}
    assert captured["authorization"] == "PDMAPIToken nexus@pam!core:secret"
    assert task == "UPID:home-pve:destroy"


@pytest.mark.asyncio
async def test_pdm_destroy_guest_rejects_non_guest_resource() -> None:
    provider = PdmProvider(base_url="https://pdm.test", verify_tls=False, health_path="/api2/json/version", timeout_seconds=5)
    with pytest.raises(RuntimeError, match="does not support resource type"):
        await provider.destroy_guest(NODE_RESOURCE)


class DecommissionProvider:
    def __init__(self, resource: InfrastructureResource) -> None:
        self.resource = resource
        self.destroyed = False
        self.stop_calls = 0
        self.destroy_calls = 0
        self.destroy_purge: bool | None = None

    async def list_resources(self) -> InfrastructureSnapshot:
        if self.destroyed:
            return InfrastructureSnapshot(())
        return InfrastructureSnapshot((self.resource,))

    async def execute_power_action(self, resource: InfrastructureResource, action: str) -> str:
        assert action == "stop"
        self.stop_calls += 1
        self.resource = dataclasses.replace(self.resource, status="stopped")
        return "UPID:stop"

    async def destroy_guest(self, resource: InfrastructureResource, *, purge: bool = True) -> str | None:
        self.destroy_calls += 1
        self.destroy_purge = purge
        self.destroyed = True
        return "UPID:destroy"


class FakeMonitoringCleanup:
    def __init__(self, *, has_target: bool = True) -> None:
        self.has_target = has_target
        self.deleted_names: list[str] = []

    async def delete_target_by_name(self, name: str) -> bool:
        self.deleted_names.append(name)
        return self.has_target


async def _no_sleep(_: float) -> None:
    return None


@pytest.mark.asyncio
async def test_decommission_disabled_by_default() -> None:
    service = InfrastructureService(DecommissionProvider(STOPPED_QEMU))
    with pytest.raises(DecommissionDisabled):
        await service.decommission(resource_id=STOPPED_QEMU.id, confirmation=STOPPED_QEMU.name)


@pytest.mark.asyncio
async def test_decommission_requires_exact_name_confirmation() -> None:
    service = InfrastructureService(DecommissionProvider(STOPPED_QEMU), decommission_enabled=True, sleep=_no_sleep)
    with pytest.raises(DecommissionNotAllowed, match="exact resource name"):
        await service.decommission(resource_id=STOPPED_QEMU.id, confirmation="wrong")


@pytest.mark.asyncio
async def test_decommission_rejects_non_guest_resources() -> None:
    service = InfrastructureService(DecommissionProvider(NODE_RESOURCE), decommission_enabled=True, sleep=_no_sleep)
    with pytest.raises(DecommissionNotAllowed, match="cannot be decommissioned"):
        await service.decommission(resource_id=NODE_RESOURCE.id, confirmation=NODE_RESOURCE.name)


@pytest.mark.asyncio
async def test_decommission_stops_running_guest_before_destroying() -> None:
    # Deliberately power_operations_enabled left at its default False: stopping a guest
    # as part of decommission must never depend on that separate flag.
    provider = DecommissionProvider(RUNNING_LXC)
    service = InfrastructureService(provider, decommission_enabled=True, verification_attempts=3, verification_interval_seconds=0, sleep=_no_sleep)
    result = await service.decommission(resource_id=RUNNING_LXC.id, confirmation=RUNNING_LXC.name)
    assert provider.stop_calls == 1
    assert provider.destroy_calls == 1
    assert result.verified is True
    assert result.task_reference == "UPID:destroy"
    assert result.monitoring_target_removed is False


@pytest.mark.asyncio
async def test_decommission_skips_stop_for_already_stopped_guest() -> None:
    provider = DecommissionProvider(STOPPED_QEMU)
    service = InfrastructureService(provider, decommission_enabled=True, verification_attempts=3, verification_interval_seconds=0, sleep=_no_sleep)
    await service.decommission(resource_id=STOPPED_QEMU.id, confirmation=STOPPED_QEMU.name)
    assert provider.stop_calls == 0
    assert provider.destroy_calls == 1


@pytest.mark.asyncio
async def test_decommission_removes_monitoring_target_when_present() -> None:
    provider = DecommissionProvider(STOPPED_QEMU)
    monitoring = FakeMonitoringCleanup(has_target=True)
    service = InfrastructureService(provider, decommission_enabled=True, verification_attempts=3, verification_interval_seconds=0, sleep=_no_sleep, monitoring_service=monitoring)
    result = await service.decommission(resource_id=STOPPED_QEMU.id, confirmation=STOPPED_QEMU.name)
    assert monitoring.deleted_names == [STOPPED_QEMU.name]
    assert result.monitoring_target_removed is True


@pytest.mark.asyncio
async def test_decommission_passes_purge_flag_through() -> None:
    provider = DecommissionProvider(STOPPED_QEMU)
    service = InfrastructureService(provider, decommission_enabled=True, verification_attempts=3, verification_interval_seconds=0, sleep=_no_sleep)
    await service.decommission(resource_id=STOPPED_QEMU.id, confirmation=STOPPED_QEMU.name, purge=False)
    assert provider.destroy_purge is False


@pytest.mark.asyncio
async def test_decommission_times_out_if_guest_never_disappears() -> None:
    class StubbornProvider(DecommissionProvider):
        async def destroy_guest(self, resource, *, purge: bool = True) -> str | None:
            self.destroy_calls += 1
            return "UPID:destroy"  # accepted, but resource never actually disappears

    provider = StubbornProvider(STOPPED_QEMU)
    service = InfrastructureService(provider, decommission_enabled=True, verification_attempts=2, verification_interval_seconds=0, sleep=_no_sleep)
    with pytest.raises(DecommissionVerificationTimeout):
        await service.decommission(resource_id=STOPPED_QEMU.id, confirmation=STOPPED_QEMU.name)


@pytest.mark.asyncio
async def test_decommission_raises_not_found_for_unknown_resource() -> None:
    service = InfrastructureService(DecommissionProvider(STOPPED_QEMU), decommission_enabled=True, sleep=_no_sleep)
    with pytest.raises(InfrastructureResourceNotFound):
        await service.decommission(resource_id="does-not-exist", confirmation="whatever")


def _app(tmp_path: Path, *, enabled: bool, resource: InfrastructureResource = STOPPED_QEMU):
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None, NEXUS_DECOMMISSION_ENABLED=enabled)
    app = create_app(settings)
    provider = DecommissionProvider(resource)
    app.state.infrastructure_service = InfrastructureService(provider, decommission_enabled=enabled, verification_attempts=3, verification_interval_seconds=0, sleep=_no_sleep)
    return app, provider


def test_decommission_api_is_locked_when_disabled(tmp_path: Path) -> None:
    app, _ = _app(tmp_path, enabled=False)
    with TestClient(app) as client:
        response = client.post("/api/v1/infrastructure/decommission", json={"resource_id": STOPPED_QEMU.id, "confirmation": STOPPED_QEMU.name})
    assert response.status_code == 403


def test_decommission_api_returns_verified_result(tmp_path: Path) -> None:
    app, provider = _app(tmp_path, enabled=True)
    with TestClient(app) as client:
        response = client.post("/api/v1/infrastructure/decommission", json={"resource_id": STOPPED_QEMU.id, "confirmation": STOPPED_QEMU.name})
    assert response.status_code == 200
    payload = response.json()
    assert payload["resource_name"] == "postgres-01"
    assert payload["verified"] is True
    assert provider.destroy_calls == 1


def test_decommission_api_rejects_wrong_confirmation(tmp_path: Path) -> None:
    app, _ = _app(tmp_path, enabled=True)
    with TestClient(app) as client:
        response = client.post("/api/v1/infrastructure/decommission", json={"resource_id": STOPPED_QEMU.id, "confirmation": "wrong"})
    assert response.status_code == 409


def test_resource_page_renders_danger_zone_locked_and_enabled(tmp_path: Path) -> None:
    locked, _ = _app(tmp_path, enabled=False, resource=RUNNING_LXC)
    with TestClient(locked) as client:
        response = client.get("/infrastructure/resource", params={"id": RUNNING_LXC.id})
    assert response.status_code == 200
    assert "Danger zone" in response.text
    assert "Decommissioning is locked" in response.text
    assert "disabled" in response.text.split("id=\"decommission-button\"")[1][:80]

    enabled, _ = _app(tmp_path, enabled=True, resource=RUNNING_LXC)
    with TestClient(enabled) as client:
        response = client.get("/infrastructure/resource", params={"id": RUNNING_LXC.id})
    assert response.status_code == 200
    assert "Decommissioning is locked" not in response.text
