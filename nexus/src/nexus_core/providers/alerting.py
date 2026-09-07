from nexus_core.ports.monitoring import MonitoringTarget


class UnconfiguredAlertingProvider:
    async def create_maintenance(self, target: MonitoringTarget) -> str:
        raise RuntimeError("NEXUS_SIGNOZ_API_KEY is not configured")

    async def delete_maintenance(self, downtime_id: str) -> None:
        raise RuntimeError("NEXUS_SIGNOZ_API_KEY is not configured")
