from __future__ import annotations

import json
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.ports.monitoring import ActiveAlert, HostSummary, MetricSample, MonitoringProviderDiagnostics, MonitoringTarget
from nexus_core.providers.file_sd import FileSdTelemetryRuntime
from nexus_core.providers.signoz import SigNozAlertingProvider, SigNozMetricsProvider
from nexus_core.repositories.monitoring_json import JsonMonitoringRepository
from nexus_core.services.monitoring import MonitoringService


class FakeAlerting:
    def __init__(self, maintained_hosts: dict[str, str] | None = None) -> None:
        self.created: list[str] = []
        self.deleted: list[str] = []
        self.host_created: list[str] = []
        self._maintained_hosts = dict(maintained_hosts or {})

    async def create_maintenance(self, target: MonitoringTarget) -> str:
        self.created.append(target.id)
        return f"dt-{target.id}"

    async def delete_maintenance(self, downtime_id: str) -> None:
        self.deleted.append(downtime_id)
        self._maintained_hosts = {name: dtid for name, dtid in self._maintained_hosts.items() if dtid != downtime_id}

    async def create_host_maintenance(self, host_name: str) -> str:
        self.host_created.append(host_name)
        downtime_id = f"dt-host-{host_name}"
        self._maintained_hosts[host_name] = downtime_id
        return downtime_id

    async def list_maintained_hosts(self) -> dict[str, str]:
        return dict(self._maintained_hosts)


class FakeMetrics:
    def __init__(
        self,
        samples: dict[str, MetricSample | None],
        hosts: list[HostSummary] | None = None,
        active_alerts: list[ActiveAlert] | None = None,
    ) -> None:
        self.samples = samples
        self.calls: list[tuple[str, str]] = []
        self.hosts = hosts or []
        self.active_alerts = active_alerts or []

    async def latest_metric(self, *, metric_name: str, filter_expression: str, lookback_seconds: int = 900):
        self.calls.append((metric_name, filter_expression))
        return self.samples.get(metric_name)

    async def diagnostics(self) -> MonitoringProviderDiagnostics:
        return MonitoringProviderDiagnostics(configured=True, healthy=True, detail="Authenticated as nexus-core")

    async def list_hosts(self) -> list[HostSummary]:
        return self.hosts

    async def list_active_alerts(self) -> list[ActiveAlert]:
        return self.active_alerts


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


def test_control_center_renders_signoz_hosts_and_active_alerts(tmp_path: Path) -> None:
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None))
    app.state.monitoring_service = MonitoringService(
        JsonMonitoringRepository(tmp_path / "monitoring.json"),
        FakeAlerting(),
        FileSdTelemetryRuntime(tmp_path / "targets.json"),
        FakeMetrics(
            {},
            hosts=[
                HostSummary(name="pve-02.internal", status="active", cpu=0.99, memory=0.56, disk_usage=0.20, load15=4.6),
                HostSummary(name="pve-01.internal", status="active", cpu=0.04, memory=0.34, disk_usage=0.05, load15=0.7),
            ],
            active_alerts=[ActiveAlert(id="r1", name="Host CPU Temperature Critical", state="firing")],
        ),
    )
    with TestClient(app) as client:
        page = client.get("/monitoring")
        assert page.status_code == 200
        assert "Infrastructure hosts" in page.text
        assert "pve-02.internal" in page.text
        assert "99%" in page.text
        assert "Host CPU Temperature Critical" in page.text
        assert "No active alerts" not in page.text


def test_control_center_shows_healthy_state_with_no_hosts_or_alerts(tmp_path: Path) -> None:
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None))
    app.state.monitoring_service = MonitoringService(
        JsonMonitoringRepository(tmp_path / "monitoring.json"),
        FakeAlerting(),
        FileSdTelemetryRuntime(tmp_path / "targets.json"),
        FakeMetrics({}),
    )
    with TestClient(app) as client:
        page = client.get("/monitoring")
        assert page.status_code == 200
        assert "No active alerts" in page.text
        assert "No hosts reported by SigNoz" in page.text


@pytest.mark.asyncio
async def test_signoz_list_hosts_parses_and_sorts_by_cpu_descending() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api/v2/infra_monitoring/hosts"
        return httpx.Response(
            200,
            json={
                "status": "success",
                "data": {
                    "type": "list",
                    "records": [
                        {"hostName": "quiet-01.internal", "status": "active", "cpu": 0.04, "memory": 0.34, "diskUsage": 0.05, "load15": 0.7},
                        {"hostName": "pve-02.internal", "status": "active", "cpu": 0.99, "memory": 0.56, "diskUsage": 0.20, "load15": 4.6},
                    ],
                    "total": 2,
                },
            },
        )

    provider = SigNozMetricsProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    hosts = await provider.list_hosts()
    assert [host.name for host in hosts] == ["pve-02.internal", "quiet-01.internal"]
    assert hosts[0].cpu == 0.99


@pytest.mark.asyncio
async def test_signoz_list_active_alerts_excludes_inactive_rules() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api/v2/rules"
        return httpx.Response(
            200,
            json={
                "status": "success",
                "data": [
                    {"id": "r1", "state": "inactive", "alert": "High Filesystem Usage Critical"},
                    {"id": "r2", "state": "firing", "alert": "Host CPU Temperature Critical"},
                    {"id": "r3", "state": "pending", "alert": "High Memory Usage"},
                ],
            },
        )

    provider = SigNozMetricsProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    alerts = await provider.list_active_alerts()
    assert {alert.id for alert in alerts} == {"r2", "r3"}
    assert all(alert.state != "inactive" for alert in alerts)


@pytest.mark.asyncio
async def test_signoz_list_hosts_treats_negative_sentinel_as_no_data() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            json={
                "status": "success",
                "data": {
                    "type": "list",
                    "records": [
                        {"hostName": "flaky-01.internal", "status": "active", "cpu": -1, "memory": 0.5, "diskUsage": -1, "load15": 0.1},
                    ],
                    "total": 1,
                },
            },
        )

    provider = SigNozMetricsProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    hosts = await provider.list_hosts()
    assert hosts[0].cpu is None
    assert hosts[0].disk_usage is None
    assert hosts[0].memory == 0.5


@pytest.mark.asyncio
async def test_signoz_create_host_maintenance_scopes_by_host_name() -> None:
    captured: list[dict[str, object]] = []

    def handler(request: httpx.Request) -> httpx.Response:
        captured.append(json.loads(request.content))
        return httpx.Response(200, json={"data": {"id": "dt-42"}})

    provider = SigNozAlertingProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    downtime_id = await provider.create_host_maintenance("pve-02.internal")
    assert downtime_id == "dt-42"
    assert captured[0]["scope"] == 'host.name="pve-02.internal"'


@pytest.mark.asyncio
async def test_signoz_list_maintained_hosts_parses_nested_shape_and_extracts_host_name() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api/v1/downtime_schedules"
        return httpx.Response(
            200,
            json={
                "data": {
                    "downtimeSchedules": [
                        {"id": "dt-1", "scope": 'host.name="pve-02.internal"'},
                        {"id": "dt-2", "scope": 'nexus_service_id="dreamteclabs-notify"'},
                    ]
                }
            },
        )

    provider = SigNozAlertingProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    mapping = await provider.list_maintained_hosts()
    assert mapping == {"pve-02.internal": "dt-1"}


@pytest.mark.asyncio
async def test_signoz_list_maintained_hosts_tolerates_flat_list_shape() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"data": [{"id": "dt-9", "scope": 'host.name="web-01.internal"'}]})

    provider = SigNozAlertingProvider(base_url="http://signoz.test", api_key="secret", transport=httpx.MockTransport(handler))
    mapping = await provider.list_maintained_hosts()
    assert mapping == {"web-01.internal": "dt-9"}


@pytest.mark.asyncio
async def test_service_list_hosts_marks_maintained_hosts(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    metrics = FakeMetrics(
        {},
        hosts=[
            HostSummary(name="pve-02.internal", status="active", cpu=0.99),
            HostSummary(name="pve-01.internal", status="active", cpu=0.04),
        ],
    )
    alerting = FakeAlerting(maintained_hosts={"pve-02.internal": "dt-1"})
    service = MonitoringService(repository, alerting, runtime, metrics)

    hosts = await service.list_hosts()
    by_name = {host.name: host for host in hosts}
    assert by_name["pve-02.internal"].maintenance is True
    assert by_name["pve-01.internal"].maintenance is False


@pytest.mark.asyncio
async def test_service_set_host_maintenance_creates_and_clears_downtime(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    alerting = FakeAlerting()
    service = MonitoringService(repository, alerting, runtime, FakeMetrics({}))

    await service.set_host_maintenance("pve-02.internal", True)
    assert alerting.host_created == ["pve-02.internal"]

    await service.set_host_maintenance("pve-02.internal", True)
    assert alerting.host_created == ["pve-02.internal"]

    await service.set_host_maintenance("pve-02.internal", False)
    assert alerting.deleted == ["dt-host-pve-02.internal"]


@pytest.mark.asyncio
async def test_service_set_host_maintenance_rejects_invalid_host_name(tmp_path: Path) -> None:
    repository = JsonMonitoringRepository(tmp_path / "monitoring.json")
    runtime = FileSdTelemetryRuntime(tmp_path / "targets.json")
    service = MonitoringService(repository, FakeAlerting(), runtime, FakeMetrics({}))

    with pytest.raises(ValueError):
        await service.set_host_maintenance('pve-02" OR 1=1--', True)


def test_control_center_host_maintenance_route_toggles_state(tmp_path: Path) -> None:
    app = create_app(Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None))
    alerting = FakeAlerting()
    app.state.monitoring_service = MonitoringService(
        JsonMonitoringRepository(tmp_path / "monitoring.json"),
        alerting,
        FileSdTelemetryRuntime(tmp_path / "targets.json"),
        FakeMetrics({}, hosts=[HostSummary(name="pve-02.internal", status="active", cpu=0.5)]),
    )
    with TestClient(app) as client:
        page = client.get("/monitoring")
        assert "Maintenance" in page.text

        response = client.patch("/api/v1/monitoring/hosts/pve-02.internal/maintenance", json={"maintenance": True})
        assert response.status_code == 200
        assert alerting.host_created == ["pve-02.internal"]

        response = client.patch("/api/v1/monitoring/hosts/pve-02.internal/maintenance", json={"maintenance": False})
        assert response.status_code == 200
        assert alerting.deleted == ["dt-host-pve-02.internal"]

        bad = client.patch("/api/v1/monitoring/hosts/not valid host/maintenance", json={"maintenance": True})
        assert bad.status_code == 422
