from __future__ import annotations

import ipaddress
import re
from dataclasses import replace

from nexus_core.ports.monitoring import AlertingProvider, MonitoringRepository, MonitoringTarget, TelemetryRuntime

_SLUG_RE = re.compile(r"[^a-z0-9]+")
_LABEL_RE = re.compile(r"^[a-z0-9_-]{1,64}$")
_PATH_RE = re.compile(r"^/[A-Za-z0-9_./-]*$")
_HOST_LABEL_RE = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$")
_VALID_STATES = {"enabled", "maintenance", "disabled"}


class MonitoringService:
    def __init__(self, repository: MonitoringRepository, alerting: AlertingProvider, telemetry: TelemetryRuntime) -> None:
        self._repository = repository
        self._alerting = alerting
        self._telemetry = telemetry

    def list_targets(self) -> list[MonitoringTarget]:
        return self._repository.list_targets()

    async def upsert_target(
        self,
        *,
        name: str,
        address: str,
        port: int,
        metrics_path: str,
        site: str,
        state: str,
    ) -> MonitoringTarget:
        normalized_name = self._normalize_name(name)
        target_id = self._slug(normalized_name)
        normalized_address = self._normalize_address(address)
        normalized_path = self._normalize_metrics_path(metrics_path)
        normalized_site = self._normalize_label(site, "site")
        normalized_state = self._normalize_state(state)
        if not 1 <= port <= 65535:
            raise ValueError("port must be between 1 and 65535")

        existing = self._repository.get_target(target_id)
        downtime_id = existing.downtime_id if existing else None

        target = MonitoringTarget(
            id=target_id,
            name=normalized_name,
            address=normalized_address,
            port=port,
            metrics_path=normalized_path,
            site=normalized_site,
            state=normalized_state,
            downtime_id=downtime_id,
        )

        if normalized_state == "maintenance" and not downtime_id:
            downtime_id = await self._alerting.create_maintenance(target)
            target = replace(target, downtime_id=downtime_id)
        elif normalized_state != "maintenance" and downtime_id:
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
