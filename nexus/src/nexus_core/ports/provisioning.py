from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Protocol


@dataclass(frozen=True, slots=True)
class ProvisioningNode:
    remote: str
    name: str
    status: str = "unknown"


@dataclass(frozen=True, slots=True)
class ProvisioningStorage:
    remote: str
    node: str | None
    name: str
    status: str = "unknown"


@dataclass(frozen=True, slots=True)
class ProvisioningNetwork:
    remote: str
    node: str | None
    name: str


@dataclass(frozen=True, slots=True)
class ProvisioningOptions:
    nodes: tuple[ProvisioningNode, ...]
    storages: tuple[ProvisioningStorage, ...]
    networks: tuple[ProvisioningNetwork, ...]
    next_vmids: dict[str, int] = field(default_factory=dict)
    used_vmids: dict[str, tuple[int, ...]] = field(default_factory=dict)


@dataclass(frozen=True, slots=True)
class GuestProvisionRequest:
    kind: str
    remote: str
    node: str
    vmid: int
    name: str
    cores: int
    memory_mb: int
    disk_gb: int
    storage: str
    bridge: str
    source: str
    ip_config: str = "dhcp"
    gateway: str | None = None
    nameserver: str | None = None
    vlan: int | None = None
    onboot: bool = True
    start: bool = True
    unprivileged: bool = True
    nesting: bool = False
    ssh_enabled: bool = False
    ssh_public_key: str | None = None
    root_password: str | None = None
    monitoring: str = "pdm"
    advanced: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True, slots=True)
class GuestCreateResult:
    task_reference: str | None
    resource_id: str


@dataclass(frozen=True, slots=True)
class ProvisioningStep:
    name: str
    status: str
    detail: str


@dataclass(frozen=True, slots=True)
class ProvisioningResult:
    resource_id: str
    resource_name: str
    status: str
    task_reference: str | None
    steps: tuple[ProvisioningStep, ...]
    warnings: tuple[str, ...] = ()


class ProvisioningProvider(Protocol):
    async def options(self) -> ProvisioningOptions: ...

    async def create_guest(self, request: GuestProvisionRequest) -> GuestCreateResult: ...

    async def storage_content(self, remote: str, node: str, storage: str, content: str) -> tuple[str, ...]: ...
