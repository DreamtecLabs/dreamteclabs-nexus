from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol, Sequence


@dataclass(frozen=True, slots=True)
class MonitoringTarget:
    id: str
    name: str
    address: str
    port: int
    metrics_path: str
    site: str
    state: str
    downtime_id: str | None = None


class MonitoringRepository(Protocol):
    def list_targets(self) -> list[MonitoringTarget]: ...

    def get_target(self, target_id: str) -> MonitoringTarget | None: ...

    def upsert_target(self, target: MonitoringTarget) -> MonitoringTarget: ...

    def delete_target(self, target_id: str) -> MonitoringTarget: ...


class AlertingProvider(Protocol):
    async def create_maintenance(self, target: MonitoringTarget) -> str: ...

    async def delete_maintenance(self, downtime_id: str) -> None: ...


class TelemetryRuntime(Protocol):
    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None: ...
