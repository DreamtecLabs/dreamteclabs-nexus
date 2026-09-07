from __future__ import annotations

from typing import Any
from urllib.parse import quote

import httpx

from nexus_core.ports.infrastructure import (
    POWER_ACTIONS,
    InfrastructureRemoteError,
    InfrastructureResource,
    InfrastructureSnapshot,
)
from nexus_core.ports.providers import ProviderStatus


class PdmProvider:
    name = "pdm"
    _resources_path = "/api2/json/resources/list"
    _canonical_types = {
        "qemu": "pve-qemu",
        "pve-qemu": "pve-qemu",
        "lxc": "pve-lxc",
        "pve-lxc": "pve-lxc",
        "storage": "pve-storage",
        "pve-storage": "pve-storage",
        "network": "pve-network",
        "pve-network": "pve-network",
        "datastore": "pbs-datastore",
        "pbs-datastore": "pbs-datastore",
        "pbs-node": "pbs-node",
        "pve-node": "pve-node",
    }

    def __init__(
        self,
        *,
        base_url: str,
        verify_tls: bool,
        health_path: str,
        timeout_seconds: float,
        api_token_id: str | None = None,
        api_token_secret: str | None = None,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._verify_tls = verify_tls
        self._health_path = "/" + health_path.lstrip("/")
        self._timeout_seconds = timeout_seconds
        self._api_token_id = api_token_id
        self._api_token_secret = api_token_secret
        self._transport = transport

    def _headers(self) -> dict[str, str]:
        if self._api_token_id and self._api_token_secret:
            return {"Authorization": f"PDMAPIToken {self._api_token_id}:{self._api_token_secret}"}
        return {}

    def _client(self) -> httpx.AsyncClient:
        return httpx.AsyncClient(
            base_url=self._base_url,
            verify=self._verify_tls,
            timeout=self._timeout_seconds,
            transport=self._transport,
            headers=self._headers(),
        )

    async def health(self) -> ProviderStatus:
        try:
            async with self._client() as client:
                response = await client.get(self._health_path)
                response.raise_for_status()
        except httpx.HTTPStatusError as exc:
            return ProviderStatus(provider=self.name, healthy=False, detail=f"HTTP {exc.response.status_code} on {self._health_path}")
        except httpx.HTTPError as exc:
            return ProviderStatus(provider=self.name, healthy=False, detail=type(exc).__name__)
        return ProviderStatus(provider=self.name, healthy=True)

    async def list_resources(self) -> InfrastructureSnapshot:
        try:
            async with self._client() as client:
                response = await client.get(self._resources_path)
                response.raise_for_status()
                payload = response.json()
        except httpx.HTTPStatusError as exc:
            raise RuntimeError(f"PDM resources API returned HTTP {exc.response.status_code}") from exc
        except httpx.HTTPError as exc:
            raise RuntimeError(f"PDM resources API failed: {type(exc).__name__}") from exc
        except ValueError as exc:
            raise RuntimeError("PDM resources API returned invalid JSON") from exc

        try:
            remote_groups = payload["data"]
            if not isinstance(remote_groups, list):
                raise TypeError("data is not a list")
            return self._normalize_resources(remote_groups)
        except (KeyError, TypeError, ValueError) as exc:
            raise RuntimeError("PDM resources API returned an unsupported payload") from exc

    async def execute_power_action(
        self,
        resource: InfrastructureResource,
        action: str,
    ) -> str | None:
        action = action.strip().lower()
        if action not in POWER_ACTIONS:
            raise RuntimeError(f"PDM power action '{action}' is not supported")
        if resource.type == "pve-qemu":
            guest_kind = "qemu"
        elif resource.type == "pve-lxc":
            guest_kind = "lxc"
        else:
            raise RuntimeError(f"PDM power operations do not support resource type '{resource.type}'")
        if resource.vmid is None:
            raise RuntimeError("PDM power operation requires a VMID")

        remote = quote(resource.remote, safe="")
        path = f"/api2/json/pve/remotes/{remote}/{guest_kind}/{resource.vmid}/{action}"
        body: dict[str, object] = {}
        if resource.node:
            body["node"] = resource.node

        try:
            async with self._client() as client:
                response = await client.post(path, json=body)
                response.raise_for_status()
                payload = response.json()
        except httpx.HTTPStatusError as exc:
            raise RuntimeError(f"PDM power API returned HTTP {exc.response.status_code} for {action}") from exc
        except httpx.HTTPError as exc:
            raise RuntimeError(f"PDM power API failed for {action}: {type(exc).__name__}") from exc
        except ValueError as exc:
            raise RuntimeError("PDM power API returned invalid JSON") from exc

        data = payload.get("data") if isinstance(payload, dict) else None
        if data is None:
            return None
        if isinstance(data, str):
            return data
        if isinstance(data, dict):
            for key in ("upid", "task", "id"):
                value = data.get(key)
                if isinstance(value, str) and value.strip():
                    return value.strip()
        return str(data)

    @classmethod
    def _normalize_resources(cls, remote_groups: list[object]) -> InfrastructureSnapshot:
        resources: list[InfrastructureResource] = []
        remote_errors: list[InfrastructureRemoteError] = []
        for raw_group in remote_groups:
            if not isinstance(raw_group, dict):
                raise TypeError("remote group is not an object")
            remote = cls._required_string(raw_group, "remote")
            error = raw_group.get("error")
            if isinstance(error, str) and error.strip():
                remote_errors.append(InfrastructureRemoteError(remote=remote, detail=error.strip()[:500]))
            raw_resources = raw_group.get("resources", [])
            if not isinstance(raw_resources, list):
                raise TypeError("resources is not a list")
            for raw_resource in raw_resources:
                if not isinstance(raw_resource, dict):
                    raise TypeError("resource is not an object")
                resources.append(cls._normalize_resource(remote, raw_resource))
        resources.sort(key=lambda item: (item.remote.casefold(), item.type, item.name.casefold(), item.id))
        remote_errors.sort(key=lambda item: item.remote.casefold())
        return InfrastructureSnapshot(tuple(resources), tuple(remote_errors))

    @classmethod
    def _normalize_resource(cls, remote: str, raw: dict[str, Any]) -> InfrastructureResource:
        raw_type = cls._required_string(raw, "type")
        resource_id = cls._required_string(raw, "id")
        resource_type = cls._canonical_resource_type(raw_type, raw)
        status = cls._optional_string(raw.get("status")) or "unknown"

        if resource_type in {"pve-qemu", "pve-lxc"}:
            name = cls._optional_string(raw.get("name")) or resource_id
        elif resource_type == "pve-node":
            name = cls._optional_string(raw.get("node")) or cls._optional_string(raw.get("name")) or resource_id
        elif resource_type == "pbs-node":
            name = cls._optional_string(raw.get("name")) or cls._optional_string(raw.get("node")) or resource_id
        elif resource_type == "pve-storage":
            name = cls._optional_string(raw.get("storage")) or cls._name_from_id(resource_id) or resource_id
        elif resource_type == "pve-network":
            name = cls._optional_string(raw.get("network")) or cls._name_from_id(resource_id) or resource_id
        elif resource_type == "pbs-datastore":
            name = cls._optional_string(raw.get("name")) or cls._name_from_id(resource_id) or resource_id
        else:
            name = cls._optional_string(raw.get("name")) or cls._name_from_id(resource_id) or resource_id

        vmid = raw.get("vmid")
        if not isinstance(vmid, int) or isinstance(vmid, bool):
            vmid = None
        template = raw.get("template")
        if not isinstance(template, bool):
            template = None

        return InfrastructureResource(
            id=resource_id,
            provider=cls.name,
            remote=remote,
            type=resource_type,
            name=name,
            status=status,
            node=cls._optional_string(raw.get("node")),
            vmid=vmid,
            template=template,
        )

    @classmethod
    def _canonical_resource_type(cls, raw_type: str, raw: dict[str, Any]) -> str:
        if raw_type == "node":
            return "pve-node" if cls._optional_string(raw.get("node")) else "pbs-node"
        return cls._canonical_types.get(raw_type, raw_type)

    @staticmethod
    def _name_from_id(resource_id: str) -> str | None:
        value = resource_id.rstrip("/").rsplit("/", 1)[-1].strip()
        return value or None

    @staticmethod
    def _required_string(raw: dict[str, Any], key: str) -> str:
        value = raw.get(key)
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"missing {key}")
        return value.strip()

    @staticmethod
    def _optional_string(value: object) -> str | None:
        if not isinstance(value, str):
            return None
        normalized = value.strip()
        return normalized or None
