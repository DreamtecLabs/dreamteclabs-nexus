from __future__ import annotations

from datetime import UTC, datetime, timedelta

import httpx

from nexus_core.ports.monitoring import MonitoringTarget


class SigNozAlertingProvider:
    def __init__(self, *, base_url: str, api_key: str, timeout_seconds: float = 5.0, transport: httpx.AsyncBaseTransport | None = None) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._timeout_seconds = timeout_seconds
        self._transport = transport

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

    async def _request(self, method: str, path: str, **kwargs: object) -> dict[str, object]:
        headers = {"SIGNOZ-API-KEY": self._api_key, "Accept": "application/json"}
        async with httpx.AsyncClient(base_url=self._base_url, headers=headers, timeout=self._timeout_seconds, transport=self._transport) as client:
            response = await client.request(method, path, **kwargs)
            response.raise_for_status()
            if not response.content:
                return {}
            data = response.json()
            if not isinstance(data, dict):
                raise RuntimeError("SigNoz returned an unexpected response")
            return data
