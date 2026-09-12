from __future__ import annotations

from dataclasses import dataclass, field
from typing import Protocol

from nexus_core.ports.infrastructure import InfrastructureResource


@dataclass(frozen=True, slots=True)
class FleetScript:
    name: str
    path: str
    description: str
    params: tuple[str, ...] = field(default_factory=tuple)


@dataclass(frozen=True, slots=True)
class FleetTarget:
    resource_id: str
    name: str
    address: str | None


@dataclass(frozen=True, slots=True)
class FleetRunResult:
    resource_id: str
    name: str
    address: str
    ok: bool
    detail: str


class FleetScriptRepository(Protocol):
    def list_scripts(self) -> list[FleetScript]: ...

    def read_script(self, path: str) -> str: ...


class FleetAddressProvider(Protocol):
    async def guest_static_address(self, resource: InfrastructureResource) -> str | None: ...
