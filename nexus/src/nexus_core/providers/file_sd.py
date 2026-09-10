from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Sequence

from nexus_core.ports.monitoring import MonitoringTarget


def _write_json(path: Path, groups: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps(groups, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


class FileSdTelemetryRuntime:
    """Publish enabled Prometheus-profile targets as Prometheus file_sd, scraped directly at their own address:port."""

    def __init__(self, path: Path) -> None:
        self._path = path

    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None:
        groups = []
        for target in sorted(targets, key=lambda item: item.id):
            if target.state != "enabled" or target.profile != "prometheus":
                continue
            groups.append(
                {
                    "targets": [f"{target.address}:{target.port}"],
                    "labels": {
                        "__metrics_path__": target.metrics_path,
                        "nexus_service_id": target.id,
                        "nexus_service_name": target.name,
                        "nexus_resource_type": "service",
                        "nexus_site": target.site,
                        "nexus_monitoring_profile": "prometheus",
                        "nexus_health_metric": target.health_metric,
                    },
                }
            )
        _write_json(self._path, groups)


class IcmpFileSdTelemetryRuntime:
    """Publish enabled icmp-profile targets as Prometheus file_sd for the Blackbox Exporter probe job.

    Unlike `FileSdTelemetryRuntime`, entries here carry only the bare address: the receiving
    scrape job (see `nexus/observability/otel-collector.yaml`) relabels `__address__` into
    `__param_target__` and rewrites `__address__` to the local Blackbox Exporter, so the target
    needs no exporter or open metrics port of its own.
    """

    def __init__(self, path: Path) -> None:
        self._path = path

    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None:
        groups = []
        for target in sorted(targets, key=lambda item: item.id):
            if target.state != "enabled" or target.profile != "icmp":
                continue
            groups.append(
                {
                    "targets": [target.address],
                    "labels": {
                        "nexus_service_id": target.id,
                        "nexus_service_name": target.name,
                        "nexus_resource_type": "service",
                        "nexus_site": target.site,
                        "nexus_monitoring_profile": "icmp",
                        "nexus_health_metric": target.health_metric,
                    },
                }
            )
        _write_json(self._path, groups)


class CompositeTelemetryRuntime:
    """Fan out reconciliation to multiple telemetry runtimes (one per monitoring profile)."""

    def __init__(self, runtimes: Sequence[object]) -> None:
        self._runtimes = tuple(runtimes)

    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None:
        for runtime in self._runtimes:
            await runtime.reconcile(targets)
