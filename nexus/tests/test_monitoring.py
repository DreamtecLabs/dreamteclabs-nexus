import json
from dataclasses import replace
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.monitoring import MonitoringTarget
from nexus_core.providers.file_sd import CompositeTelemetryRuntime, FileSdTelemetryRuntime, IcmpFileSdTelemetryRuntime
from nexus_core.providers.signoz import SigNozAlertingProvider
from nexus_core.repositories.monitoring_json import JsonMonitoringRepository
from nexus_core.services.monitoring import MonitoringService


class FakeAlerting:
    def __init__(self) -> None:
        self.created: list[str] = []
        self.deleted: list[str] = []

    async def create_maintenance(self, target: MonitoringTarget) -> str:
        self.created.append(target.id)
        return f"dt-{target.id}"

    async def delete_maintenance(self, downtime_id: str) -> None:
        self.deleted.append(downtime_id)


@pytest.mark.asyncio
async def test_service_lifecycle_writes_file_sd_and_maintenance(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    alerting = FakeAlerting()
    service = MonitoringService(repository, alerting, runtime)

    enabled = await service.upsert_target(
        name="DreamtecLabs Notify",
        address="notify-01",
        port=8000,
        metrics_path="/metrics",
        site="home",
        state="enabled",
    )
    assert enabled.id == "dreamteclabs-notify"
    groups = json.loads((tmp_path / "targets.json").read_text())
    assert groups[0]["targets"] == ["notify-01:8000"]
    assert groups[0]["labels"]["nexus_service_id"] == "dreamteclabs-notify"

    maintenance = await service.upsert_target(
        name="DreamtecLabs Notify",
        address="notify-01",
        port=8000,
        metrics_path="/metrics",
        site="home",
        state="maintenance",
    )
    assert maintenance.downtime_id == "dt-dreamteclabs-notify"
    assert alerting.created == ["dreamteclabs-notify"]
    assert json.loads((tmp_path / "targets.json").read_text()) == []

    resumed = await service.upsert_target(
        name="DreamtecLabs Notify",
        address="notify-01",
        port=8000,
        metrics_path="/metrics",
        site="home",
        state="enabled",
    )
    assert resumed.downtime_id is None
    assert alerting.deleted == ["dt-dreamteclabs-notify"]


@pytest.mark.asyncio
async def test_delete_target_by_name_removes_a_registered_target(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    service = MonitoringService(repository, FakeAlerting(), runtime)
    await service.upsert_target(name="demo-01", address="192.168.0.50", profile="icmp", site="home", state="enabled")

    removed = await service.delete_target_by_name("demo-01")

    assert removed is True
    assert repository.get_target("demo-01") is None


@pytest.mark.asyncio
async def test_delete_target_by_name_is_a_noop_when_nothing_registered(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    service = MonitoringService(repository, FakeAlerting(), runtime)

    removed = await service.delete_target_by_name("never-existed")

    assert removed is False


@pytest.mark.asyncio
async def test_icmp_profile_targets_need_no_port_and_route_to_the_icmp_file_sd(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = CompositeTelemetryRuntime(
        [
            FileSdTelemetryRuntime(tmp_path / "targets.json"),
            IcmpFileSdTelemetryRuntime(tmp_path / "targets-icmp.json"),
        ]
    )
    service = MonitoringService(repository, FakeAlerting(), runtime)

    target = await service.upsert_target(
        name="Zigbee Coordinator",
        address="192.168.0.60",
        site="home",
        state="enabled",
        profile="icmp",
        health_metric="probe_success",
    )
    assert target.port is None
    assert target.metrics_path is None
    assert target.profile == "icmp"

    prometheus_groups = json.loads((tmp_path / "targets.json").read_text())
    assert prometheus_groups == []
    icmp_groups = json.loads((tmp_path / "targets-icmp.json").read_text())
    assert icmp_groups[0]["targets"] == ["192.168.0.60"]
    assert icmp_groups[0]["labels"]["nexus_service_id"] == "zigbee-coordinator"
    assert icmp_groups[0]["labels"]["nexus_monitoring_profile"] == "icmp"
    assert icmp_groups[0]["labels"]["nexus_health_metric"] == "probe_success"


@pytest.mark.asyncio
async def test_invalid_profile_is_rejected() -> None:
    service = MonitoringService(
        JsonMonitoringRepository(Path("/dev/null")),
        FakeAlerting(),
        FileSdTelemetryRuntime(Path("/dev/null")),
    )
    with pytest.raises(ValueError):
        await service.upsert_target(name="bad", address="192.168.0.1", site="home", state="enabled", profile="not-a-profile")


@pytest.mark.asyncio
async def test_prometheus_profile_requires_a_port() -> None:
    service = MonitoringService(
        JsonMonitoringRepository(Path("/dev/null")),
        FakeAlerting(),
        FileSdTelemetryRuntime(Path("/dev/null")),
    )
    with pytest.raises(ValueError):
        await service.upsert_target(name="bad", address="192.168.0.1", site="home", state="enabled")


@pytest.mark.asyncio
async def test_service_rejects_shell_like_addresses(tmp_path: Path) -> None:
    service = MonitoringService(
        JsonMonitoringRepository(tmp_path / "monitoring.json"),
        FakeAlerting(),
        FileSdTelemetryRuntime(tmp_path / "targets.json"),
    )
    with pytest.raises(ValueError):
        await service.upsert_target(
            name="bad",
            address="127.0.0.1;id",
            port=8000,
            metrics_path="/metrics",
            site="home",
            state="enabled",
        )


@pytest.mark.asyncio
async def test_signoz_maintenance_scope_is_service_specific() -> None:
    captured: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["json"] = json.loads(request.content)
        return httpx.Response(200, json={"data": {"id": "dt-42"}})

    provider = SigNozAlertingProvider(
        base_url="http://signoz.test",
        api_key="secret",
        transport=httpx.MockTransport(handler),
    )
    target = MonitoringTarget(
        id="dreamteclabs-notify",
        name="DreamtecLabs Notify",
        address="notify-01",
        port=8000,
        metrics_path="/metrics",
        site="home",
        state="maintenance",
    )
    assert await provider.create_maintenance(target) == "dt-42"
    assert captured["json"]["scope"] == 'nexus_service_id="dreamteclabs-notify"'


def test_api_manages_targets_without_touching_pdm(tmp_path: Path) -> None:
    settings = Settings(
        NEXUS_DATA_DIR=tmp_path,
        PDM_BASE_URL="https://pdm.invalid",
        PDM_VERIFY_TLS=False,
        NEXUS_SIGNOZ_API_KEY=None,
    )
    client = TestClient(create_app(settings))
    response = client.put(
        "/api/v1/monitoring/targets",
        json={
            "name": "DreamtecLabs Notify",
            "address": "notify-01",
            "port": 8000,
            "metrics_path": "/metrics",
            "site": "home",
            "state": "enabled",
        },
    )
    assert response.status_code == 200
    assert response.json()["id"] == "dreamteclabs-notify"
    listing = client.get("/api/v1/monitoring/targets")
    assert listing.json()["count"] == 1


def test_api_accepts_icmp_targets_without_a_port(tmp_path: Path) -> None:
    settings = Settings(
        NEXUS_DATA_DIR=tmp_path,
        PDM_BASE_URL="https://pdm.invalid",
        PDM_VERIFY_TLS=False,
        NEXUS_SIGNOZ_API_KEY=None,
    )
    client = TestClient(create_app(settings))
    response = client.put(
        "/api/v1/monitoring/targets",
        json={
            "name": "Zigbee Coordinator",
            "address": "192.168.0.60",
            "site": "home",
            "state": "enabled",
            "profile": "icmp",
            "health_metric": "probe_success",
        },
    )
    assert response.status_code == 200
    body = response.json()
    assert body["profile"] == "icmp"
    assert body["port"] is None
    assert body["metrics_path"] is None
