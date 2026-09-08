from __future__ import annotations

import json
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.monitoring import MetricSample, MonitoringProviderDiagnostics, MonitoringTarget
from nexus_core.providers.file_sd import FileSdTelemetryRuntime
from nexus_core.providers.signoz import SigNozMetricsProvider
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


class FakeMetrics:
    def __init__(self, samples: dict[str, MetricSample | None]) -> None:
        self.samples = samples
        self.calls: list[tuple[str, str]] = []

    async def latest_metric(self, *, metric_name: str, filter_expression: str, lookback_seconds: int = 900):
        self.calls.append((metric_name, filter_expression))
        return self.samples.get(metric_name)

    async def diagnostics(self) -> MonitoringProviderDiagnostics:
        return MonitoringProviderDiagnostics(configured=True, healthy=True, detail="Authenticated as nexus-core")


@pytest.mark.asyncio
async def test_monitoring_status_respects_lifecycle_and_health_metric(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    metrics = FakeMetrics({"service_up": MetricSample(value=1.0, timestamp_ms=1788816900000)})
    service = MonitoringService(repository, FakeAlerting(), runtime, metrics)

    target = await service.upsert_target(
        name="DreamtecLabs Notify",
        address="192.168.0.37",
        port=8000,
        metrics_path="/metrics",
        site="home",
        state="enabled",
        health_metric="service_up",
    )
    status = await service.get_status(target)
    assert status.status == "healthy"
    assert status.value == 1.0
    assert metrics.calls == [("service_up", 'nexus_service_id = "dreamteclabs-notify"')]

    maintenance = await service.set_state(target.id, "maintenance")
    maintenance_status = await service.get_status(maintenance)
    assert maintenance_status.status == "maintenance"
    assert len(metrics.calls) == 1
    assert json.loads((tmp_path / "targets.json").read_text()) == []

    disabled = await service.set_state(target.id, "disabled")
    assert (await service.get_status(disabled)).status == "disabled"
    assert len(metrics.calls) == 1


@pytest.mark.asyncio
async def test_monitoring_status_handles_down_missing_and_provider_errors(tmp_path: Path) -> None:
    class ErrorMetrics(FakeMetrics):
        async def latest_metric(self, *, metric_name: str, filter_expression: str, lookback_seconds: int = 900):
            if metric_name == "broken_metric":
                raise RuntimeError("SigNoz returned HTTP 503 for /api/v5/query_range")
            return await super().latest_metric(metric_name=metric_name, filter_expression=filter_expression)

    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    metrics = ErrorMetrics({"down_metric": MetricSample(value=0.0, timestamp_ms=1000), "missing_metric": None})
    service = MonitoringService(repository, FakeAlerting(), runtime, metrics)

    statuses = []
    for name, metric in (("Down", "down_metric"), ("Missing", "missing_metric"), ("Broken", "broken_metric")):
        target = await service.upsert_target(name=name, address="127.0.0.1", port=9000, metrics_path="/metrics", site="home", state="enabled", health_metric=metric)
        statuses.append(await service.get_status(target))

    assert [status.status for status in statuses] == ["down", "unknown", "unknown"]
    assert "No recent" in (statuses[1].detail or "")
    assert "HTTP 503" in (statuses[2].detail or "")
    assert service.summarize(statuses) == {"total": 3, "healthy": 0, "down": 1, "unknown": 2, "maintenance": 0, "disabled": 0}


@pytest.mark.asyncio
async def test_signoz_v5_query_and_service_account_diagnostics() -> None:
    captured: list[tuple[str, str, dict[str, object] | None]] = []

    def handler(request: httpx.Request) -> httpx.Response:
        body = json.loads(request.content) if request.content else None
        captured.append((request.method, request.url.path, body))
        assert request.headers.get("SIGNOZ-API-KEY") == "secret"
        if request.url.path == "/api/v1/service_accounts/me":
            return httpx.Response(200, json={"data": {"name": "nexus-core", "status": "active"}})
        return httpx.Response(
            200,
            json={"status": "success", "data": {"data": {"results": [{"aggregations": [{"series": [{"values": [{"timestamp": 1788816840000, "value": 0}, {"timestamp": 1788816900000, "value": 1}]}]}]}]}}},
        )

    provider = SigNozMetricsProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    sample = await provider.latest_metric(metric_name="dreamteclabs_notify_evolution_up", filter_expression='nexus_service_id = "dreamteclabs-notify"')
    assert sample == MetricSample(value=1.0, timestamp_ms=1788816900000)
    payload = captured[0][2]
    assert payload is not None
    spec = payload["compositeQuery"]["queries"][0]["spec"]
    assert spec["aggregations"][0]["metricName"] == "dreamteclabs_notify_evolution_up"
    assert spec["aggregations"][0]["spaceAggregation"] == "avg"
    assert spec["filter"]["expression"] == 'nexus_service_id = "dreamteclabs-notify"'

    diagnostics = await provider.diagnostics()
    assert diagnostics.healthy is True
    assert diagnostics.detail == "Authenticated as nexus-core"


@pytest.mark.asyncio
async def test_signoz_errors_do_not_leak_response_body() -> None:
    def handler(_: httpx.Request) -> httpx.Response:
        return httpx.Response(403, text="super-secret-backend-detail")

    provider = SigNozMetricsProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    with pytest.raises(RuntimeError, match="HTTP 403") as exc_info:
        await provider.latest_metric(metric_name="up", filter_expression='nexus_service_id = "service"')
    assert "super-secret" not in str(exc_info.value)
    diagnostics = await provider.diagnostics()
    assert diagnostics.configured is True
    assert diagnostics.healthy is False
    assert "HTTP 403" in (diagnostics.detail or "")
    assert "super-secret" not in (diagnostics.detail or "")


def test_control_center_api_and_ui(tmp_path: Path) -> None:
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None))
    repository = JsonMonitoringRepository(tmp_path / "monitoring-ui.json")
    service = MonitoringService(
        repository,
        FakeAlerting(),
        FileSdTelemetryRuntime(tmp_path / "targets-ui.json"),
        FakeMetrics({"up": MetricSample(value=1.0, timestamp_ms=1788816900000)}),
    )
    app.state.monitoring_service = service

    with TestClient(app) as client:
        created = client.put("/api/v1/monitoring/targets", json={"name": "Notify", "address": "192.168.0.37", "port": 8000, "metrics_path": "/metrics", "site": "home", "state": "enabled", "health_metric": "up"})
        assert created.status_code == 200

        status = client.get("/api/v1/monitoring/status")
        assert status.status_code == 200
        assert status.json()["summary"]["healthy"] == 1
        assert status.json()["provider"]["healthy"] is True

        page = client.get("/monitoring")
        assert page.status_code == 200
        assert "Monitoring Control Center" in page.text
        assert "Notify" in page.text
        assert "healthy" in page.text
        assert "Maintenance" in page.text

        changed = client.patch("/api/v1/monitoring/targets/notify/state", json={"state": "maintenance"})
        assert changed.status_code == 200
        assert changed.json()["state"] == "maintenance"
        status_after = client.get("/api/v1/monitoring/targets/notify/status")
        assert status_after.json()["status"] == "maintenance"
