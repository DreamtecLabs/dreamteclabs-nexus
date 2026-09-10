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


@dataclass(frozen=True, slots=True)
class HostSummary:
    """A host SigNoz's infrastructure-monitoring agent already tracks (not a Nexus-managed target)."""

    name: str
    status: str
    cpu: float | None = None
    memory: float | None = None
    disk_usage: float | None = None
    load15: float | None = None
    maintenance: bool = False


@dataclass(frozen=True, slots=True)
class ActiveAlert:
    """A SigNoz alert rule currently outside its inactive/OK state (pending or firing)."""

    id: str
    name: str
    state: str


class MonitoringRepository(Protocol):
    def list_targets(self) -> list[MonitoringTarget]: ...

    def get_target(self, target_id: str) -> MonitoringTarget | None: ...

    def upsert_target(self, target: MonitoringTarget) -> MonitoringTarget: ...

    def delete_target(self, target_id: str) -> MonitoringTarget: ...


class AlertingProvider(Protocol):
    async def create_maintenance(self, target: MonitoringTarget) -> str: ...

    async def delete_maintenance(self, downtime_id: str) -> None: ...

    async def create_host_maintenance(self, host_name: str) -> str: ...

    async def list_maintained_hosts(self) -> dict[str, str]:
        """Map SigNoz-agent host name -> downtime id, for hosts Nexus put into maintenance."""
        ...


class MetricsProvider(Protocol):
    async def latest_metric(
        self,
        *,
        metric_name: str,
        filter_expression: str,
        lookback_seconds: int = 900,
    ) -> MetricSample | None: ...

    async def diagnostics(self) -> MonitoringProviderDiagnostics: ...

    async def list_hosts(self) -> list[HostSummary]: ...

    async def list_active_alerts(self) -> list[ActiveAlert]: ...


class TelemetryRuntime(Protocol):
    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None: ...
