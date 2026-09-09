from __future__ import annotations

import json
from typing import Any
from urllib.parse import quote

import httpx

from nexus_core.ports.provisioning import (
    GuestCreateResult,
    GuestProvisionRequest,
    ProvisioningNetwork,
    ProvisioningNode,
    ProvisioningOptions,
    ProvisioningStorage,
)


class PdmProvisioningProvider:
    """PDM-backed guest creation adapter.

    Nexus keeps the familiar PVE creation model but all mutations go through PDM.
    The PDM node create endpoints forward native PVE creation parameters, allowing
    advanced PVE options without introducing direct PVE access in Nexus.
    """

    _NEXUS_METADATA_KEYS = {
        "monitoring_address",
        "monitoring_port",
        "monitoring_path",
        "monitoring_site",
        "health_metric",
    }

    def __init__(self, *, base_url: str, verify_tls: bool, timeout_seconds: float, api_token_id: str | None = None, api_token_secret: str | None = None, transport: httpx.AsyncBaseTransport | None = None) -> None:
        self._base_url = base_url.rstrip("/")
        self._verify_tls = verify_tls
        self._timeout_seconds = timeout_seconds
        self._api_token_id = api_token_id
        self._api_token_secret = api_token_secret
        self._transport = transport

    def _headers(self) -> dict[str, str]:
        if self._api_token_id and self._api_token_secret:
            return {"Authorization": f"PDMAPIToken {self._api_token_id}:{self._api_token_secret}"}
        return {}

    def _client(self) -> httpx.AsyncClient:
        return httpx.AsyncClient(base_url=self._base_url, verify=self._verify_tls, timeout=self._timeout_seconds, transport=self._transport, headers=self._headers())

    async def options(self) -> ProvisioningOptions:
        try:
            async with self._client() as client:
                response = await client.get("/api2/json/resources/list")
                response.raise_for_status()
                groups = response.json().get("data", [])
                nodes: list[ProvisioningNode] = []
                storages: list[ProvisioningStorage] = []
                networks: list[ProvisioningNetwork] = []
                remotes: set[str] = set()
                for group in groups:
                    if not isinstance(group, dict):
                        continue
                    remote = str(group.get("remote", "")).strip()
                    if not remote:
                        continue
                    resources = group.get("resources", [])
                    for item in resources if isinstance(resources, list) else []:
                        if not isinstance(item, dict):
                            continue
                        raw_type = str(item.get("type", ""))
                        node = self._text(item.get("node"))
                        if raw_type in {"node", "pve-node"} and node:
                            remotes.add(remote)
                            nodes.append(ProvisioningNode(remote, node, self._text(item.get("status")) or "unknown"))
                        elif raw_type in {"storage", "pve-storage"}:
                            name = self._text(item.get("storage")) or self._tail(self._text(item.get("id")))
                            if name:
                                storages.append(ProvisioningStorage(remote, node, name, self._text(item.get("status")) or "unknown"))
                        elif raw_type in {"network", "pve-network"}:
                            name = self._text(item.get("network")) or self._tail(self._text(item.get("id")))
                            if name:
                                networks.append(ProvisioningNetwork(remote, node, name))
                next_vmids: dict[str, int] = {}
                for remote in sorted(remotes):
                    next_response = await client.get(f"/api2/json/pve/remotes/{quote(remote, safe='')}/cluster-nextid")
                    if next_response.is_success:
                        try:
                            next_vmids[remote] = int(next_response.json().get("data"))
                        except (TypeError, ValueError):
                            pass
        except httpx.HTTPStatusError as exc:
            raise RuntimeError(f"PDM provisioning options returned HTTP {exc.response.status_code}") from exc
        except httpx.HTTPError as exc:
            raise RuntimeError(f"PDM provisioning options failed: {type(exc).__name__}") from exc
        except ValueError as exc:
            raise RuntimeError("PDM provisioning options returned invalid JSON") from exc
        nodes.sort(key=lambda item: (item.remote.casefold(), item.name.casefold()))
        storages.sort(key=lambda item: (item.remote.casefold(), (item.node or "").casefold(), item.name.casefold()))
        networks.sort(key=lambda item: (item.remote.casefold(), (item.node or "").casefold(), item.name.casefold()))
        return ProvisioningOptions(tuple(nodes), tuple(storages), tuple(networks), next_vmids)

    async def create_guest(self, request: GuestProvisionRequest) -> GuestCreateResult:
        config = self._native_config(request)
        remote = quote(request.remote, safe="")
        node = quote(request.node, safe="")
        endpoint = "create-lxc" if request.kind == "lxc" else "create-qemu"
        path = f"/api2/json/pve/remotes/{remote}/nodes/{node}/{endpoint}"
        try:
            async with self._client() as client:
                response = await client.post(path, json={"config": json.dumps(config, separators=(",", ":"))})
                response.raise_for_status()
                payload = response.json()
        except httpx.HTTPStatusError as exc:
            raise RuntimeError(f"PDM guest creation returned HTTP {exc.response.status_code}") from exc
        except httpx.HTTPError as exc:
            raise RuntimeError(f"PDM guest creation failed: {type(exc).__name__}") from exc
        except ValueError as exc:
            raise RuntimeError("PDM guest creation returned invalid JSON") from exc
        task = payload.get("data") if isinstance(payload, dict) else None
        task_reference = task if isinstance(task, str) else json.dumps(task) if task is not None else None
        return GuestCreateResult(task_reference=task_reference, resource_id=f"remote/{request.remote}/guest/{request.vmid}")

    @classmethod
    def _native_config(cls, request: GuestProvisionRequest) -> dict[str, Any]:
        if request.kind not in {"lxc", "qemu"}:
            raise ValueError("kind must be 'lxc' or 'qemu'")
        if request.vmid < 100 or request.vmid > 999_999_999:
            raise ValueError("VMID is outside the PVE range")
        if not request.name.strip() or not request.source.strip():
            raise ValueError("guest name and template/ISO source are required")
        if request.cores < 1 or request.memory_mb < 128 or request.disk_gb < 1:
            raise ValueError("CPU, memory and disk values must be positive")
        net_parts = ["name=eth0" if request.kind == "lxc" else "virtio", f"bridge={request.bridge}"]
        if request.vlan is not None:
            net_parts.append(f"tag={request.vlan}")
        if request.kind == "lxc":
            net_parts.append(f"ip={request.ip_config or 'dhcp'}")
            if request.gateway:
                net_parts.append(f"gw={request.gateway}")
            config: dict[str, Any] = {"vmid": request.vmid, "hostname": request.name.strip(), "ostemplate": request.source.strip(), "cores": request.cores, "memory": request.memory_mb, "rootfs": f"{request.storage}:{request.disk_gb}", "net0": ",".join(net_parts), "onboot": 1 if request.onboot else 0, "unprivileged": 1 if request.unprivileged else 0, "start": 1 if request.start else 0}
            if request.nameserver:
                config["nameserver"] = request.nameserver
            if request.nesting:
                config["features"] = "nesting=1"
            if request.ssh_enabled and request.ssh_public_key:
                config["ssh-public-keys"] = request.ssh_public_key.strip()
        else:
            config = {"vmid": request.vmid, "name": request.name.strip(), "cores": request.cores, "memory": request.memory_mb, "scsihw": "virtio-scsi-pci", "scsi0": f"{request.storage}:{request.disk_gb}", "net0": ",".join(net_parts), "ide2": f"{request.source.strip()},media=cdrom", "agent": 1, "onboot": 1 if request.onboot else 0, "start": 1 if request.start else 0}
        protected = {"vmid"}
        for key, value in request.advanced.items():
            normalized = str(key).strip()
            if normalized and normalized not in protected and normalized not in cls._NEXUS_METADATA_KEYS and value is not None:
                config[normalized] = value
        return config

    @staticmethod
    def _text(value: object) -> str | None:
        if not isinstance(value, str):
            return None
        value = value.strip()
        return value or None

    @staticmethod
    def _tail(value: str | None) -> str | None:
        if not value:
            return None
        return value.rstrip("/").rsplit("/", 1)[-1] or None
