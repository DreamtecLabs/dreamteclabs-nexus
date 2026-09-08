from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Sequence

from nexus_core.ports.monitoring import MonitoringTarget


class FileSdTelemetryRuntime:
    """Publish enabled targets as Prometheus file_sd without managing PDM or systemd."""

    def __init__(self, path: Path) -> None:
        self._path = path

    async def reconcile(self, targets: Sequence[MonitoringTarget]) -> None:
        groups = []
        for target in sorted(targets, key=lambda item: item.id):
            if target.state != "enabled":
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

        self._path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self._path.with_suffix(self._path.suffix + f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(groups, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(self._path)
