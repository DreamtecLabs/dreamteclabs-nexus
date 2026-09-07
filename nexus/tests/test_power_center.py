from __future__ import annotations

from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.providers.pdm import PdmProvider
from nexus_core.services.infrastructure import (
    InfrastructureService,
    PowerActionNotAllowed,
    PowerOperationsDisabled,
    PowerVerificationTimeout,
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


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("resource", "action", "expected_path"),
    [
        (RUNNING_LXC, "shutdown", "/api2/json/pve/remotes/home-pve/lxc/104/shutdown"),
        (RUNNING_LXC, "stop", "/api2/json/pve/remotes/home-pve/lxc/104/stop"),
        (STOPPED_QEMU, "start", "/api2/json/pve/remotes/home-pve/qemu/101/start"),
    ],
)
async def test_pdm_power_routes_are_exact_and_authenticated(resource, action, expected_path) -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["method"] = request.method
        captured["path"] = request.url.path
        captured["authorization"] = request.headers.get("Authorization")
        captured["body"] = request.read().decode()
        return httpx.Response(200, json={"data": "UPID:home-pve:0001"})

    provider = PdmProvider(
        base_url="https://pdm.test",
        verify_tls=False,
        health_path="/api2/json/version",
        timeout_seconds=5,
        api_token_id="nexus@pam!core",
        api_token_secret="secret",
        transport=httpx.MockTransport(handler),
    )
    task = await provider.execute_power_action(resource, action)

    assert captured["method"] == "POST"
    assert captured["path"] == expected_path
    assert captured["authorization"] == "PDMAPIToken nexus@pam!core:secret"
    assert '"node":"' in str(captured["body"])
    assert task == "UPID:home-pve:0001"


@pytest.mark.asyncio
async def test_pdm_power_http_failure_does_not_expose_response_body() -> None:
    def handler(_: httpx.Request) -> httpx.Response:
        return httpx.Response(403, text="sensitive upstream detail")

    provider = PdmProvider(
        base_url="https://pdm.test",
        verify_tls=False,
        health_path="/api2/json/version",
        timeout_seconds=5,
        transport=httpx.MockTransport(handler),
    )
    with pytest.raises(RuntimeError, match="PDM power API returned HTTP 403") as exc_info:
        await provider.execute_power_action(RUNNING_LXC, "shutdown")
    assert "sensitive upstream detail" not in str(exc_info.value)


class StatefulProvider:
    def __init__(self, resource: InfrastructureResource, *, converge: bool = True) -> None:
        self.resource = resource
        self.converge = converge
        self.calls: list[str] = []
        self.reads = 0

    async def list_resources(self) -> InfrastructureSnapshot:
        self.reads += 1
        resource = self.resource
        if self.calls and self.converge:
            action = self.calls[-1]
            expected = "running" if action == "start" else "stopped"
            if self.reads >= 3:
                resource = InfrastructureResource(
                    id=resource.id,
                    provider=resource.provider,
                    remote=resource.remote,
                    type=resource.type,
                    name=resource.name,
                    status=expected,
                    node=resource.node,
                    vmid=resource.vmid,
                    template=resource.template,
                )
                self.resource = resource
        return InfrastructureSnapshot((resource,))

    async def execute_power_action(self, resource: InfrastructureResource, action: str) -> str:
        assert resource.id == self.resource.id
        self.calls.append(action)
        return "UPID:test"


async def _no_sleep(_: float) -> None:
    return None


@pytest.mark.asyncio
async def test_service_blocks_mutations_by_default() -> None:
    service = InfrastructureService(StatefulProvider(STOPPED_QEMU))
    with pytest.raises(PowerOperationsDisabled):
        await service.execute_power_action(resource_id=STOPPED_QEMU.id, action="start")


@pytest.mark.asyncio
async def test_service_allows_only_state_valid_actions() -> None:
    service = InfrastructureService(StatefulProvider(STOPPED_QEMU), power_operations_enabled=True, sleep=_no_sleep)
    with pytest.raises(PowerActionNotAllowed, match="shutdown is not available"):
        await service.execute_power_action(resource_id=STOPPED_QEMU.id, action="shutdown")


@pytest.mark.asyncio
async def test_hard_stop_requires_exact_resource_name() -> None:
    service = InfrastructureService(StatefulProvider(RUNNING_LXC), power_operations_enabled=True, sleep=_no_sleep)
    with pytest.raises(PowerActionNotAllowed, match="exact resource name"):
        await service.execute_power_action(resource_id=RUNNING_LXC.id, action="stop", confirmation="wrong")


@pytest.mark.asyncio
async def test_service_verifies_final_state_after_provider_acceptance() -> None:
    provider = StatefulProvider(STOPPED_QEMU)
    service = InfrastructureService(
        provider,
        power_operations_enabled=True,
        verification_attempts=4,
        verification_interval_seconds=0,
        sleep=_no_sleep,
    )
    result = await service.execute_power_action(resource_id=STOPPED_QEMU.id, action="start")
    assert result.verified is True
    assert result.observed_status == "running"
    assert result.task_reference == "UPID:test"
    assert provider.calls == ["start"]
    assert provider.reads >= 3


@pytest.mark.asyncio
async def test_service_does_not_report_success_without_readback_convergence() -> None:
    provider = StatefulProvider(STOPPED_QEMU, converge=False)
    service = InfrastructureService(
        provider,
        power_operations_enabled=True,
        verification_attempts=2,
        verification_interval_seconds=0,
        sleep=_no_sleep,
    )
    with pytest.raises(PowerVerificationTimeout, match="did not observe status 'running'"):
        await service.execute_power_action(resource_id=STOPPED_QEMU.id, action="start")


def _app(tmp_path: Path, *, enabled: bool):
    settings = Settings(
        NEXUS_DATA_DIR=tmp_path,
        PDM_BASE_URL="https://pdm.invalid",
        PDM_VERIFY_TLS=False,
        NEXUS_SIGNOZ_API_KEY=None,
        NEXUS_POWER_OPERATIONS_ENABLED=enabled,
    )
    app = create_app(settings)
    provider = StatefulProvider(STOPPED_QEMU)
    app.state.infrastructure_service = InfrastructureService(
        provider,
        power_operations_enabled=enabled,
        verification_attempts=4,
        verification_interval_seconds=0,
        sleep=_no_sleep,
    )
    return app


def test_power_api_is_locked_when_configuration_is_disabled(tmp_path: Path) -> None:
    app = _app(tmp_path, enabled=False)
    with TestClient(app) as client:
        response = client.post("/api/v1/infrastructure/power", json={"resource_id": STOPPED_QEMU.id, "action": "start"})
    assert response.status_code == 403
    assert "disabled by configuration" in response.json()["detail"]


def test_power_api_returns_verified_operation(tmp_path: Path) -> None:
    app = _app(tmp_path, enabled=True)
    with TestClient(app) as client:
        response = client.post("/api/v1/infrastructure/power", json={"resource_id": STOPPED_QEMU.id, "action": "start"})
    assert response.status_code == 200
    payload = response.json()
    assert payload["resource_name"] == "postgres-01"
    assert payload["action"] == "start"
    assert payload["observed_status"] == "running"
    assert payload["verified"] is True


def test_power_center_renders_locked_and_enabled_action_state(tmp_path: Path) -> None:
    locked = _app(tmp_path, enabled=False)
    with TestClient(locked) as client:
        response = client.get("/infrastructure/power")
    assert response.status_code == 200
    assert "Resource Power Center" in response.text
    assert "Power operations are locked" in response.text
    assert "postgres-01" in response.text
    assert 'data-action="start" disabled' in response.text

    enabled = _app(tmp_path, enabled=True)
    with TestClient(enabled) as client:
        response = client.get("/infrastructure/power")
    assert response.status_code == 200
    assert 'data-action="start" disabled' not in response.text
    assert 'data-action="shutdown" disabled' in response.text
