from __future__ import annotations

from datetime import UTC, datetime, timedelta
import re
import time
from typing import Any

import httpx

from nexus_core.ports.monitoring import ActiveAlert, HostSummary, MetricSample, MonitoringProviderDiagnostics, MonitoringTarget


class _SigNozClient:
    def __init__(
        self,
        *,
        base_url: str,
        api_key: str,
        timeout_seconds: float = 5.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._timeout_seconds = timeout_seconds
        self._transport = transport

    async def _request(self, method: str, path: str, **kwargs: object) -> dict[str, Any]:
        headers = {"SIGNOZ-API-KEY": self._api_key, "Accept": "application/json"}
        try:
            async with httpx.AsyncClient(
                base_url=self._base_url,
                headers=headers,
                timeout=self._timeout_seconds,
                transport=self._transport,
            ) as client:
                response = await client.request(method, path, **kwargs)
                response.raise_for_status()
        except httpx.HTTPStatusError as exc:
            raise RuntimeError(f"SigNoz returned HTTP {exc.response.status_code} for {path}") from exc
        except httpx.HTTPError as exc:
            raise RuntimeError(f"SigNoz request failed: {type(exc).__name__}") from exc
        if not response.content:
            return {}
        try:
            data = response.json()
        except ValueError as exc:
            raise RuntimeError("SigNoz returned invalid JSON") from exc
        if not isinstance(data, dict):
            raise RuntimeError("SigNoz returned an unexpected response")
        return data


_HOST_SCOPE_RE = re.compile(r'host\.name\s*=\s*"([^"]+)"')


class SigNozAlertingProvider(_SigNozClient):
    async def create_maintenance(self, target: MonitoringTarget) -> str:
        return await self._create_downtime(
            name=f"Nexus maintenance: {target.name}",
            description=f"Automatically created by Nexus while service '{target.id}' is in Maintenance.",
            scope=f'nexus_service_id="{target.id}"',
        )

    async def create_host_maintenance(self, host_name: str) -> str:
        return await self._create_downtime(
            name=f"Nexus maintenance: {host_name}",
            description=f"Automatically created by Nexus while host '{host_name}' is in Maintenance.",
            scope=f'host.name="{host_name}"',
        )

    async def _create_downtime(self, *, name: str, description: str, scope: str) -> str:
        now = datetime.now(UTC)
        payload = {
            "name": name,
            "description": description,
            "schedule": {
                "timezone": "UTC",
                "startTime": now.isoformat().replace("+00:00", "Z"),
                "endTime": (now + timedelta(days=3650)).isoformat().replace("+00:00", "Z"),
            },
            "alertIds": [],
            "scope": scope,
        }
        data = await self._request("POST", "/api/v1/downtime_schedules", json=payload)
        for path in (("id",), ("data", "id"), ("data", "downtimeSchedule", "id"), ("downtimeSchedule", "id")):
            value: object = data
            for key in path:
                if not isinstance(value, dict) or key not in value:
                    break
                value = value[key]
            else:
                if isinstance(value, (str, int)):
                    return str(value)
        raise RuntimeError("SigNoz did not return a recognizable downtime id")

    async def delete_maintenance(self, downtime_id: str) -> None:
        await self._request("DELETE", f"/api/v1/downtime_schedules/{downtime_id}")

    async def list_maintained_hosts(self) -> dict[str, str]:
        data = await self._request("GET", "/api/v1/downtime_schedules")
        records = self._extract_list(
            data,
            (("data", "downtimeSchedules"), ("data", "downtime_schedules"), ("downtimeSchedules",), ("downtime_schedules",), ("data",)),
        )
        mapping: dict[str, str] = {}
        for record in records:
            if not isinstance(record, dict):
                continue
            downtime_id = record.get("id")
            scope = record.get("scope")
            if downtime_id is None or not isinstance(scope, str):
                continue
            match = _HOST_SCOPE_RE.search(scope)
            if match:
                mapping[match.group(1)] = str(downtime_id)
        return mapping

    @staticmethod
    def _extract_list(data: object, paths: tuple[tuple[str, ...], ...]) -> list[object]:
        for path in paths:
            value: object = data
            for key in path:
                if not isinstance(value, dict) or key not in value:
                    value = None
                    break
                value = value[key]
            if isinstance(value, list):
                return value
        return []


class SigNozMetricsProvider(_SigNozClient):
    async def latest_metric(
        self,
        *,
        metric_name: str,
        filter_expression: str,
        lookback_seconds: int = 900,
    ) -> MetricSample | None:
        end_ms = int(time.time() * 1000)
        start_ms = end_ms - max(60, lookback_seconds) * 1000
        payload = {
            "start": start_ms,
            "end": end_ms,
            "requestType": "time_series",
            "compositeQuery": {
                "queries": [
                    {
                        "type": "builder_query",
                        "spec": {
                            "name": "A",
                            "signal": "metrics",
                            "stepInterval": 60,
                            "aggregations": [
                                {
                                    "metricName": metric_name,
                                    "timeAggregation": "avg",
                                    "spaceAggregation": "avg",
                                }
                            ],
                            "filter": {"expression": filter_expression},
                            "disabled": False,
                        },
                    }
                ]
            },
        }
        data = await self._request("POST", "/api/v5/query_range", json=payload)
        points = self._metric_points(data)
        if not points:
            return None
        return max(points, key=lambda point: point.timestamp_ms)

    async def list_hosts(self, *, lookback_seconds: int = 120, limit: int = 200) -> list[HostSummary]:
        # SigNoz's own Hosts UI defaults to a 30-minute window, but its cpu/memory/disk
        # figures are an average over [start, end], not an instantaneous read. A page meant
        # to show "what's happening right now" needs a short window instead, or a host that
        # spiked earlier and already recovered still shows as critical minutes later.
        end_ms = int(time.time() * 1000)
        start_ms = end_ms - max(60, lookback_seconds) * 1000
        payload = {
            "filter": {"expression": ""},
            "offset": 0,
            "limit": limit,
            "start": start_ms,
            "end": end_ms,
        }
        data = await self._request("POST", "/api/v2/infra_monitoring/hosts", json=payload)
        body = data.get("data")
        records = body.get("records") if isinstance(body, dict) else None
        hosts = [
            HostSummary(
                name=str(record.get("hostName", "unknown")),
                status=str(record.get("status", "unknown")),
                cpu=self._as_float(record.get("cpu")),
                memory=self._as_float(record.get("memory")),
                disk_usage=self._as_float(record.get("diskUsage")),
                load15=self._as_float(record.get("load15")),
            )
            for record in (records or [])
            if isinstance(record, dict)
        ]
        hosts.sort(key=lambda host: host.cpu if host.cpu is not None else -1.0, reverse=True)
        return hosts

    async def list_active_alerts(self) -> list[ActiveAlert]:
        data = await self._request("GET", "/api/v2/rules")
        records = data.get("data")
        alerts: list[ActiveAlert] = []
        for record in records if isinstance(records, list) else []:
            if not isinstance(record, dict):
                continue
            state = str(record.get("state", "")).casefold()
            if state in ("", "inactive"):
                continue
            alerts.append(ActiveAlert(id=str(record.get("id", "")), name=str(record.get("alert", "unnamed rule")), state=state))
        return alerts

    @staticmethod
    def _as_float(value: object) -> float | None:
        # SigNoz returns a negative sentinel (observed: -1) when a host doesn't have
        # enough samples in the window for a stable rate() calculation. Treat that as
        # "no data" rather than displaying a nonsensical negative percentage.
        if not isinstance(value, (int, float)):
            return None
        result = float(value)
        return result if result >= 0 else None

    async def diagnostics(self) -> MonitoringProviderDiagnostics:
        try:
            data = await self._request("GET", "/api/v1/service_accounts/me")
        except RuntimeError as exc:
            return MonitoringProviderDiagnostics(configured=True, healthy=False, detail=str(exc))
        account = data.get("data", data)
        if not isinstance(account, dict):
            return MonitoringProviderDiagnostics(configured=True, healthy=False, detail="SigNoz service account response is invalid")
        status = str(account.get("status", "")).casefold()
        name = account.get("name") or account.get("email") or "service account"
        if status and status != "active":
            return MonitoringProviderDiagnostics(configured=True, healthy=False, detail=f"SigNoz {name} is {status}")
        return MonitoringProviderDiagnostics(configured=True, healthy=True, detail=f"Authenticated as {name}")

    @classmethod
    def _metric_points(cls, payload: object) -> list[MetricSample]:
        points: list[MetricSample] = []

        def normalize_timestamp(value: object) -> int | None:
            if not isinstance(value, (int, float)):
                return None
            timestamp = int(value)
            if timestamp < 10_000_000_000:
                timestamp *= 1000
            return timestamp

        def visit(value: object) -> None:
            if isinstance(value, dict):
                raw_value = value.get("value")
                raw_timestamp = value.get("timestamp", value.get("timestamp_ms", value.get("time")))
                timestamp = normalize_timestamp(raw_timestamp)
                if isinstance(raw_value, (int, float)) and timestamp is not None:
                    points.append(MetricSample(value=float(raw_value), timestamp_ms=timestamp))
                for child in value.values():
                    visit(child)
            elif isinstance(value, list):
                if len(value) >= 2:
                    timestamp = normalize_timestamp(value[0])
                    sample_value = value[1]
                    if timestamp is not None and isinstance(sample_value, (int, float)):
                        points.append(MetricSample(value=float(sample_value), timestamp_ms=timestamp))
                for child in value:
                    visit(child)

        visit(payload)
        unique = {(point.timestamp_ms, point.value): point for point in points}
        return list(unique.values())
