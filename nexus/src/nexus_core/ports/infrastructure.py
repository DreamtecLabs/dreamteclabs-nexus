from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol


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


@dataclass(frozen=True, slots=True)
class InfrastructureRemoteError:
    remote: str
    detail: str


@dataclass(frozen=True, slots=True)
class InfrastructureSnapshot:
    resources: tuple[InfrastructureResource, ...]
    remote_errors: tuple[InfrastructureRemoteError, ...] = ()


class InfrastructureProvider(Protocol):
    async def list_resources(self) -> InfrastructureSnapshot: ...
