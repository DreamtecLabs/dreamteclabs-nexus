from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol


PVE_GUEST_TYPES = frozenset({"pve-lxc", "pve-qemu"})
POWER_ACTIONS = frozenset({"start", "shutdown", "stop"})


@dataclass(frozen=True, slots=True)
class InfrastructureResource:
    id: str
    provider: str
    remote: str
    type: str
    name: str
    status: str
    node: str | None = None
    vmid: int | None = None
    template: bool | None = None
    cpu_usage: float | None = None
    cpu_total: float | None = None
    memory_used_bytes: int | None = None
    memory_total_bytes: int | None = None
    disk_used_bytes: int | None = None
    disk_total_bytes: int | None = None
    uptime_seconds: int | None = None

    @property
    def is_guest(self) -> bool:
        return self.type in PVE_GUEST_TYPES and self.vmid is not None


@dataclass(frozen=True, slots=True)
class InfrastructureRemoteError:
    remote: str
    detail: str


@dataclass(frozen=True, slots=True)
class InfrastructureSnapshot:
    resources: tuple[InfrastructureResource, ...]
    remote_errors: tuple[InfrastructureRemoteError, ...] = ()


@dataclass(frozen=True, slots=True)
class InfrastructureResourceContext:
    resource: InfrastructureResource
    node: InfrastructureResource | None = None
    guests: tuple[InfrastructureResource, ...] = ()
    storages: tuple[InfrastructureResource, ...] = ()
    networks: tuple[InfrastructureResource, ...] = ()
    related: tuple[InfrastructureResource, ...] = ()


@dataclass(frozen=True, slots=True)
class InfrastructureNodeSummary:
    name: str
    status: str
    guests: int
    running: int
    stopped: int
    cpu_usage: float | None = None
    cpu_total: float | None = None
    memory_used_bytes: int | None = None
    memory_total_bytes: int | None = None
    uptime_seconds: int | None = None


@dataclass(frozen=True, slots=True)
class InfrastructureRemoteSummary:
    remote: str
    resources: int
    guests: int
    running: int
    stopped: int
    pve_nodes: int
    pbs_resources: int
    storages: int
    networks: int
    nodes: tuple[InfrastructureNodeSummary, ...] = ()


@dataclass(frozen=True, slots=True)
class PowerOperationResult:
    resource_id: str
    resource_name: str
    action: str
    task_reference: str | None
    expected_status: str
    observed_status: str
    verified: bool


@dataclass(frozen=True, slots=True)
class PowerAuditEntry:
    timestamp: str
    resource_id: str
    resource_name: str
    action: str
    result: str
    detail: str
    task_reference: str | None = None


@dataclass(frozen=True, slots=True)
class DecommissionResult:
    resource_id: str
    resource_name: str
    task_reference: str | None
    verified: bool
    monitoring_target_removed: bool


class InfrastructureProvider(Protocol):
    async def list_resources(self) -> InfrastructureSnapshot: ...

    async def execute_power_action(
        self,
        resource: InfrastructureResource,
        action: str,
    ) -> str | None: ...

    async def destroy_guest(
        self,
        resource: InfrastructureResource,
        *,
        purge: bool = True,
    ) -> str | None: ...


class PowerAuditRepository(Protocol):
    def record(self, entry: PowerAuditEntry) -> None: ...

    def list_recent(self, limit: int = 25) -> tuple[PowerAuditEntry, ...]: ...
