from __future__ import annotations

from datetime import UTC, datetime, timedelta
import time
from typing import Any

import httpx

from nexus_core.ports.monitoring import MetricSample, MonitoringProviderDiagnostics, MonitoringTarget


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


class SigNozAlertingProvider(_SigNozClient):
    async def create_maintenance(self, target: MonitoringTarget) -> str:
        now = datetime.now(UTC)
        payload = {
            "name": f"Nexus maintenance: {target.name}",
            "description": f"Automatically created by Nexus while service '{target.id}' is in Maintenance.",
            "schedule": {
                "timezone": "UTC",
                "startTime": now.isoformat().replace("+00:00", "Z"),
                "endTime": (now + timedelta(days=3650)).isoformat().replace("+00:00", "Z"),
            },
            "alertIds": [],
            "scope": f'nexus_service_id="{target.id}"',
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
