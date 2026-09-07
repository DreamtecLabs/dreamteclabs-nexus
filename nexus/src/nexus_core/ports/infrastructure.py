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
class PowerOperationResult:
    resource_id: str
    resource_name: str
    action: str
    task_reference: str | None
    expected_status: str
    observed_status: str
    verified: bool


class InfrastructureProvider(Protocol):
    async def list_resources(self) -> InfrastructureSnapshot: ...

    async def execute_power_action(
        self,
        resource: InfrastructureResource,
        action: str,
    ) -> str | None: ...
