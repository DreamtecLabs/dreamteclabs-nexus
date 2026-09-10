from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol, Sequence


@dataclass(frozen=True, slots=True)
class MonitoringTarget:
    id: str
    name: str
    address: str
    site: str
    state: str
    port: int | None = None
    metrics_path: str | None = None
    profile: str = "prometheus"
    downtime_id: str | None = None
    health_metric: str = "up"


@dataclass(frozen=True, slots=True)
class MetricSample:
    value: float
    timestamp_ms: int


@dataclass(frozen=True, slots=True)
class MonitoringStatus:
    target_id: str
    target_name: str
    state: str
    status: str
    metric: str
    value: float | None = None
    timestamp_ms: int | None = None
    detail: str | None = None


@dataclass(frozen=True, slots=True)
class MonitoringProviderDiagnostics:
    configured: bool
    healthy: bool
    detail: str | None = None


class MonitoringRepository(Protocol):
    def list_targets(self) -> list[MonitoringTarget]: ...

    def get_target(self, target_id: str) -> MonitoringTarget | None: ...

    def upsert_target(self, target: MonitoringTarget) -> MonitoringTarget: ...

    def delete_target(self, target_id: str) -> MonitoringTarget: ...


class AlertingProvider(Protocol):
    async def create_maintenance(self, target: MonitoringTarget) -> str: ...

    async def delete_maintenance(self, downtime_id: str) -> None: ...


class MetricsProvider(Protocol):
    async def latest_metric(
        self,
        *,
        metric_name: str,
        filter_expression: str,
        lookback_seconds: int = 900,
    ) -> MetricSample | None: ...

    async def diagnostics(self) -> MonitoringProviderDiagnostics: ...


class TelemetryRuntime(Protocol):
    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None: ...
