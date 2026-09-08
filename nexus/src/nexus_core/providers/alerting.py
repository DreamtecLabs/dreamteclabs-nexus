from nexus_core.ports.monitoring import MetricSample, MonitoringProviderDiagnostics, MonitoringTarget


class UnconfiguredAlertingProvider:
    async def create_maintenance(self, target: MonitoringTarget) -> str:
        raise RuntimeError("NEXUS_SIGNOZ_API_KEY is not configured")

    async def delete_maintenance(self, downtime_id: str) -> None:
        raise RuntimeError("NEXUS_SIGNOZ_API_KEY is not configured")


class UnconfiguredMetricsProvider:
    async def latest_metric(
        self,
        *,
        metric_name: str,
        filter_expression: str,
        lookback_seconds: int = 900,
    ) -> MetricSample | None:
        return None

    async def diagnostics(self) -> MonitoringProviderDiagnostics:
        return MonitoringProviderDiagnostics(
            configured=False,
            healthy=False,
            detail="NEXUS_SIGNOZ_API_KEY is not configured",
        )
