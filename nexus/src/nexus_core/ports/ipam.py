from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from nexus_core.ports.infrastructure import InfrastructureResource


@dataclass(frozen=True, slots=True)
class IpamEntry:
    address: str
    source: str  # "guest" | "manual"
    label: str
    resource_id: str | None = None
    notes: str = ""


@dataclass(frozen=True, slots=True)
class IpamConflict:
    address: str
    guest_entry: IpamEntry
    manual_entry: IpamEntry


@dataclass(frozen=True, slots=True)
class IpamSnapshot:
    cidr: str
    dhcp_range: tuple[str, str]
    used: tuple[IpamEntry, ...]
    free: tuple[str, ...]
    conflicts: tuple[IpamConflict, ...] = ()


class IpamRepository(Protocol):
    def list_entries(self) -> list[IpamEntry]: ...

    def get_entry(self, address: str) -> IpamEntry | None: ...

    def upsert_entry(self, entry: IpamEntry) -> IpamEntry: ...

    def delete_entry(self, address: str) -> IpamEntry: ...


class GuestAddressProvider(Protocol):
    async def guest_static_address(self, resource: InfrastructureResource) -> str | None: ...
