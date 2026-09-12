from __future__ import annotations

from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.infrastructure import InfrastructureResource, InfrastructureSnapshot
from nexus_core.repositories.fleet_scripts import FilesystemFleetScriptRepository
from nexus_core.services.fleet import FleetOperationsDisabled, FleetService
from nexus_core.services.ssh_bootstrap import BootstrapResult

LXC_RUNNING = InfrastructureResource(id="remote/homelab/guest/101", provider="pdm", remote="homelab", type="pve-lxc", name="web-01", status="running", node="pve-01", vmid=101)
LXC_STOPPED = InfrastructureResource(id="remote/homelab/guest/102", provider="pdm", remote="homelab", type="pve-lxc", name="db-01", status="stopped", node="pve-01", vmid=102)
QEMU_RUNNING = InfrastructureResource(id="remote/homelab/guest/103", provider="pdm", remote="homelab", type="pve-qemu", name="windows", status="running", node="pve-01", vmid=103)


def _write_script(directory: Path, filename: str, *, name: str, description: str = "", params: str = "", body: str = "echo hi") -> Path:
    path = directory / filename
    path.write_text(f"#!/usr/bin/env bash\n# nexus-fleet:name={name}\n# nexus-fleet:description={description}\n# nexus-fleet:params={params}\nset -euo pipefail\n{body}\n")
    return path


def test_repository_lists_only_marked_scripts(tmp_path: Path) -> None:
    _write_script(tmp_path, "otel.sh", name="Install OTel Collector agent", description="Installs otelcol", params="OTEL_VERSION,OTLP_HOST")
    (tmp_path / "nexus-domains-helper").write_text("#!/usr/bin/env bash\necho not fleet-marked\n")
    repository = FilesystemFleetScriptRepository(tmp_path)

    scripts = repository.list_scripts()

    assert len(scripts) == 1
    assert scripts[0].name == "Install OTel Collector agent"
    assert scripts[0].path == "otel.sh"
    assert scripts[0].params == ("OTEL_VERSION", "OTLP_HOST")


def test_repository_read_script_returns_content(tmp_path: Path) -> None:
    _write_script(tmp_path, "timezone.sh", name="Set timezone", body="echo setting timezone")
    repository = FilesystemFleetScriptRepository(tmp_path)

    body = repository.read_script("timezone.sh")

    assert "echo setting timezone" in body


def test_repository_read_script_rejects_path_traversal(tmp_path: Path) -> None:
    _write_script(tmp_path, "timezone.sh", name="Set timezone")
    repository = FilesystemFleetScriptRepository(tmp_path)

    with pytest.raises(KeyError):
        repository.read_script("../secrets.sh")


def test_repository_missing_directory_returns_empty(tmp_path: Path) -> None:
    repository = FilesystemFleetScriptRepository(tmp_path / "does-not-exist")
    assert repository.list_scripts() == []


class FakeInfrastructure:
    def __init__(self, resources=()) -> None:
        self.resources = tuple(resources)

    async def list_resources(self) -> InfrastructureSnapshot:
        return InfrastructureSnapshot(self.resources)


class FakeAddresses:
    def __init__(self, mapping: dict[str, str] | None = None) -> None:
        self.mapping = mapping or {}

    async def guest_static_address(self, resource: InfrastructureResource) -> str | None:
        return self.mapping.get(resource.id)


class FakeSshBootstrap:
    def __init__(self, *, ok: bool = True, detail: str = "bootstrap script completed") -> None:
        self.ok = ok
        self.detail = detail
        self.run_calls: list[dict[str, object]] = []
        self.enroll_calls: list[dict[str, object]] = []

    async def wait_and_run(self, address: str, *, username: str, script: str, env: dict[str, str]) -> BootstrapResult:
        self.run_calls.append({"address": address, "username": username, "script": script, "env": env})
        return BootstrapResult(self.ok, self.detail)

    async def enroll_key(self, address: str, *, username: str, password: str) -> BootstrapResult:
        self.enroll_calls.append({"address": address, "username": username, "password": password})
        return BootstrapResult(self.ok, self.detail)


@pytest.mark.asyncio
async def test_list_targets_filters_to_running_lxc_only() -> None:
    infra = FakeInfrastructure([LXC_RUNNING, LXC_STOPPED, QEMU_RUNNING])
    addresses = FakeAddresses({LXC_RUNNING.id: "192.168.0.10"})
    service = FleetService(FilesystemFleetScriptRepository(Path("/nonexistent")), infra, addresses, FakeSshBootstrap(), enabled=True)

    targets = await service.list_targets()

    assert len(targets) == 1
    assert targets[0].resource_id == LXC_RUNNING.id
    assert targets[0].address == "192.168.0.10"


@pytest.mark.asyncio
async def test_enroll_key_rejects_when_disabled() -> None:
    service = FleetService(FilesystemFleetScriptRepository(Path("/nonexistent")), FakeInfrastructure(), FakeAddresses(), FakeSshBootstrap(), enabled=False)
    with pytest.raises(FleetOperationsDisabled):
        await service.enroll_key(address="192.168.0.10", password="secret")


@pytest.mark.asyncio
async def test_run_rejects_when_disabled(tmp_path: Path) -> None:
    _write_script(tmp_path, "timezone.sh", name="Set timezone", params="TIMEZONE")
    service = FleetService(FilesystemFleetScriptRepository(tmp_path), FakeInfrastructure(), FakeAddresses(), FakeSshBootstrap(), enabled=False)
    with pytest.raises(FleetOperationsDisabled):
        await service.run(script="timezone.sh", targets=[{"resource_id": "x", "name": "x", "address": "1.2.3.4"}], params={})


@pytest.mark.asyncio
async def test_run_unknown_script_raises_key_error(tmp_path: Path) -> None:
    service = FleetService(FilesystemFleetScriptRepository(tmp_path), FakeInfrastructure(), FakeAddresses(), FakeSshBootstrap(), enabled=True)
    with pytest.raises(KeyError):
        await service.run(script="missing.sh", targets=[{"resource_id": "x", "name": "x", "address": "1.2.3.4"}], params={})


@pytest.mark.asyncio
async def test_run_executes_against_every_target_and_forwards_params(tmp_path: Path) -> None:
    _write_script(tmp_path, "timezone.sh", name="Set timezone", params="TIMEZONE")
    ssh_bootstrap = FakeSshBootstrap()
    service = FleetService(FilesystemFleetScriptRepository(tmp_path), FakeInfrastructure(), FakeAddresses(), ssh_bootstrap, enabled=True)

    results = await service.run(
        script="timezone.sh",
        targets=[
            {"resource_id": LXC_RUNNING.id, "name": "web-01", "address": "192.168.0.10"},
            {"resource_id": "remote/homelab/guest/104", "name": "db-01", "address": "192.168.0.11"},
        ],
        params={"TIMEZONE": "America/Sao_Paulo"},
    )

    assert len(results) == 2
    assert {r.address for r in results} == {"192.168.0.10", "192.168.0.11"}
    assert all(r.ok for r in results)
    assert all(call["env"] == {"TIMEZONE": "America/Sao_Paulo"} for call in ssh_bootstrap.run_calls)


def _app(tmp_path: Path, *, enabled: bool = True):
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None, NEXUS_FLEET_OPERATIONS_ENABLED=enabled)
    app = create_app(settings)
    _write_script(tmp_path, "timezone.sh", name="Set timezone", description="Sets the timezone", params="TIMEZONE")
    app.state.fleet_service = FleetService(FilesystemFleetScriptRepository(tmp_path), FakeInfrastructure([LXC_RUNNING]), FakeAddresses({LXC_RUNNING.id: "192.168.0.10"}), FakeSshBootstrap(), enabled=enabled)
    return app


def test_fleet_scripts_route_lists_discovered_scripts(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/api/v1/fleet/scripts")
    assert response.status_code == 200
    scripts = response.json()["scripts"]
    assert scripts == [{"name": "Set timezone", "path": "timezone.sh", "description": "Sets the timezone", "params": ["TIMEZONE"]}]


def test_fleet_targets_route_lists_running_lxc(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/api/v1/fleet/targets")
    assert response.status_code == 200
    assert response.json()["targets"] == [{"resource_id": LXC_RUNNING.id, "name": "web-01", "address": "192.168.0.10"}]


def test_fleet_enroll_key_route(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.post("/api/v1/fleet/enroll-key", json={"address": "192.168.0.10", "password": "hunter2"})
    assert response.status_code == 200
    assert response.json()["status"] == "ok"


def test_fleet_enroll_key_route_disabled(tmp_path: Path) -> None:
    app = _app(tmp_path, enabled=False)
    with TestClient(app) as client:
        response = client.post("/api/v1/fleet/enroll-key", json={"address": "192.168.0.10", "password": "hunter2"})
    assert response.status_code == 403


def test_fleet_run_route(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.post("/api/v1/fleet/run", json={
            "script": "timezone.sh",
            "targets": [{"resource_id": LXC_RUNNING.id, "name": "web-01", "address": "192.168.0.10"}],
            "params": {"TIMEZONE": "America/Sao_Paulo"},
        })
    assert response.status_code == 200
    results = response.json()["results"]
    assert results == [{"resource_id": LXC_RUNNING.id, "name": "web-01", "address": "192.168.0.10", "ok": True, "detail": "bootstrap script completed"}]


def test_fleet_run_route_unknown_script(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.post("/api/v1/fleet/run", json={
            "script": "missing.sh",
            "targets": [{"resource_id": LXC_RUNNING.id, "name": "web-01", "address": "192.168.0.10"}],
            "params": {},
        })
    assert response.status_code == 404


def test_fleet_page_renders(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.get("/fleet")
    assert response.status_code == 200
    assert "Fleet Ops" in response.text
    assert "Set timezone" in response.text
    assert "web-01" in response.text
