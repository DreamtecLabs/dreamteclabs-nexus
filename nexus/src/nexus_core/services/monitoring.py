from __future__ import annotations

import asyncio
import ipaddress
import re
from dataclasses import replace

from nexus_core.ports.monitoring import (
    ActiveAlert,
    AlertingProvider,
    HostSummary,
    MetricsProvider,
    MonitoringProviderDiagnostics,
    MonitoringRepository,
    MonitoringStatus,
    MonitoringTarget,
    TelemetryRuntime,
)

_SLUG_RE = re.compile(r"[^a-z0-9]+")
_LABEL_RE = re.compile(r"^[a-z0-9_-]{1,64}$")
_METRIC_RE = re.compile(r"^[A-Za-z_:][A-Za-z0-9_:]{0,127}$")
_PATH_RE = re.compile(r"^/[A-Za-z0-9_./-]*$")
_HOST_LABEL_RE = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$")
_VALID_STATES = {"enabled", "maintenance", "disabled"}
_VALID_PROFILES = {"prometheus", "icmp"}


class MonitoringService:
    def __init__(
        self,
        repository: MonitoringRepository,
        alerting: AlertingProvider,
        telemetry: TelemetryRuntime,
        metrics: MetricsProvider | None = None,
    ) -> None:
        self._repository = repository
        self._alerting = alerting
        self._telemetry = telemetry
        self._metrics = metrics

    def list_targets(self) -> list[MonitoringTarget]:
        return self._repository.list_targets()

    async def provider_diagnostics(self) -> MonitoringProviderDiagnostics:
        if self._metrics is None:
            return MonitoringProviderDiagnostics(configured=False, healthy=False, detail="Metrics provider is not configured")
        return await self._metrics.diagnostics()

    async def list_hosts(self) -> list[HostSummary]:
        # Read-only visibility into every host SigNoz's own infra-monitoring agent tracks,
        # independent of Nexus-managed targets. A failure here is surfaced via the existing
        # provider_diagnostics() card rather than raising through the page.
        if self._metrics is None:
            return []
        try:
            return await self._metrics.list_hosts()
        except RuntimeError:
            return []

    async def list_active_alerts(self) -> list[ActiveAlert]:
        if self._metrics is None:
            return []
        try:
            return await self._metrics.list_active_alerts()
        except RuntimeError:
            return []

    async def get_status(self, target: MonitoringTarget) -> MonitoringStatus:
        if target.state == "disabled":
            return MonitoringStatus(target.id, target.name, target.state, "disabled", target.health_metric)
        if target.state == "maintenance":
            return MonitoringStatus(target.id, target.name, target.state, "maintenance", target.health_metric)
        if self._metrics is None:
            return MonitoringStatus(
                target.id,
                target.name,
                target.state,
                "unknown",
                target.health_metric,
                detail="Metrics provider is not configured",
            )
        try:
            sample = await self._metrics.latest_metric(
                metric_name=target.health_metric,
                filter_expression=f'nexus_service_id = "{target.id}"',
            )
        except RuntimeError as exc:
            return MonitoringStatus(
                target.id,
                target.name,
                target.state,
                "unknown",
                target.health_metric,
                detail=str(exc),
            )
        if sample is None:
            return MonitoringStatus(
                target.id,
                target.name,
                target.state,
                "unknown",
                target.health_metric,
                detail="No recent SigNoz samples",
            )
        return MonitoringStatus(
            target.id,
            target.name,
            target.state,
            "healthy" if sample.value > 0 else "down",
            target.health_metric,
            value=sample.value,
            timestamp_ms=sample.timestamp_ms,
        )

    async def list_statuses(self) -> list[MonitoringStatus]:
        targets = self.list_targets()
        if not targets:
            return []
        return list(await asyncio.gather(*(self.get_status(target) for target in targets)))

    @staticmethod
    def summarize(statuses: list[MonitoringStatus]) -> dict[str, int]:
        summary = {"total": len(statuses), "healthy": 0, "down": 0, "unknown": 0, "maintenance": 0, "disabled": 0}
        for status in statuses:
            if status.status in summary:
                summary[status.status] += 1
        return summary

    async def upsert_target(
        self,
        *,
        name: str,
        address: str,
        site: str,
        state: str,
        profile: str = "prometheus",
        port: int | None = None,
        metrics_path: str | None = None,
        health_metric: str = "up",
    ) -> MonitoringTarget:
        normalized_profile = self._normalize_profile(profile)
        normalized_name = self._normalize_name(name)
        target_id = self._slug(normalized_name)
        normalized_address = self._normalize_address(address)
        normalized_site = self._normalize_label(site, "site")
        normalized_state = self._normalize_state(state)
        normalized_metric = self._normalize_metric(health_metric)
        if normalized_profile == "icmp":
            normalized_port = None
            normalized_path = None
        else:
            if port is None or not 1 <= port <= 65535:
                raise ValueError("port must be between 1 and 65535")
            normalized_port = port
            normalized_path = self._normalize_metrics_path(metrics_path or "/metrics")

        existing = self._repository.get_target(target_id)
        downtime_id = existing.downtime_id if existing else None

        target = MonitoringTarget(
            id=target_id,
            name=normalized_name,
            address=normalized_address,
            port=normalized_port,
            metrics_path=normalized_path,
            profile=normalized_profile,
            site=normalized_site,
            state=normalized_state,
            downtime_id=downtime_id,
            health_metric=normalized_metric,
        )
        return await self._apply_lifecycle(target, downtime_id)

    async def set_state(self, target_id: str, state: str) -> MonitoringTarget:
        existing = self._repository.get_target(target_id)
        if existing is None:
            raise KeyError(target_id)
        target = replace(existing, state=self._normalize_state(state))
        return await self._apply_lifecycle(target, existing.downtime_id)

    async def _apply_lifecycle(self, target: MonitoringTarget, downtime_id: str | None) -> MonitoringTarget:
        if target.state == "maintenance" and not downtime_id:
            downtime_id = await self._alerting.create_maintenance(target)
            target = replace(target, downtime_id=downtime_id)
        elif target.state != "maintenance" and downtime_id:
            await self._alerting.delete_maintenance(downtime_id)
            target = replace(target, downtime_id=None)

        stored = self._repository.upsert_target(target)
        await self._telemetry.reconcile(self._repository.list_targets())
        return stored

    async def delete_target(self, target_id: str) -> MonitoringTarget:
        existing = self._repository.get_target(target_id)
        if existing is None:
            raise KeyError(target_id)
        if existing.downtime_id:
            await self._alerting.delete_maintenance(existing.downtime_id)
        deleted = self._repository.delete_target(target_id)
        await self._telemetry.reconcile(self._repository.list_targets())
        return deleted

    @staticmethod
    def _normalize_name(value: str) -> str:
        normalized = value.strip()
        if not 1 <= len(normalized) <= 80 or any(ord(ch) < 32 for ch in normalized):
            raise ValueError("name must contain between 1 and 80 printable characters")
        return normalized

    @staticmethod
    def _slug(value: str) -> str:
        slug = _SLUG_RE.sub("-", value.casefold()).strip("-")
        if not slug or len(slug) > 64:
            raise ValueError("name cannot be converted to a valid target id")
        return slug

    @staticmethod
    def _normalize_label(value: str, field: str) -> str:
        normalized = value.strip().casefold()
        if not _LABEL_RE.fullmatch(normalized):
            raise ValueError(f"{field} must contain 1-64 alphanumeric, '-' or '_' characters")
        return normalized

    @staticmethod
    def _normalize_metric(value: str) -> str:
        normalized = value.strip()
        if not _METRIC_RE.fullmatch(normalized):
            raise ValueError("health_metric must be a valid Prometheus metric name")
        return normalized

    @staticmethod
    def _normalize_address(value: str) -> str:
        normalized = value.strip().rstrip(".").casefold()
        try:
            ipaddress.ip_address(normalized)
            return normalized
        except ValueError:
            pass
        if not normalized or len(normalized) > 253 or any(not _HOST_LABEL_RE.fullmatch(label) for label in normalized.split(".")):
            raise ValueError("invalid IP address or hostname")
        return normalized

    @staticmethod
    def _normalize_metrics_path(value: str) -> str:
        normalized = value.strip() or "/metrics"
        if len(normalized) > 128 or not _PATH_RE.fullmatch(normalized):
            raise ValueError("metrics_path must be a safe absolute path")
        return normalized

    @staticmethod
    def _normalize_state(value: str) -> str:
        normalized = value.strip().casefold()
        if normalized not in _VALID_STATES:
            raise ValueError("state must be enabled, maintenance or disabled")
        return normalized

    @staticmethod
    def _normalize_profile(value: str) -> str:
        normalized = value.strip().casefold()
        if normalized not in _VALID_PROFILES:
            raise ValueError("profile must be prometheus or icmp")
        return normalized
