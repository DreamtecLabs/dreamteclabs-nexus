from __future__ import annotations

import asyncio
import ipaddress

from nexus_core.ports.ipam import GuestAddressProvider, IpamConflict, IpamEntry, IpamRepository, IpamSnapshot


class IpamService:
    def __init__(
        self,
        repository: IpamRepository,
        infrastructure_service,
        guest_address_provider: GuestAddressProvider,
        *,
        cidr: str = "192.168.0.0/24",
        dhcp_range_start: str = "192.168.0.50",
        dhcp_range_end: str = "192.168.0.199",
    ) -> None:
        self._repository = repository
        self._infrastructure = infrastructure_service
        self._guest_address_provider = guest_address_provider
        self._network = ipaddress.ip_network(cidr, strict=False)
        self._dhcp_start = ipaddress.ip_address(dhcp_range_start)
        self._dhcp_end = ipaddress.ip_address(dhcp_range_end)

    def _in_dhcp_range(self, address: ipaddress.IPv4Address | ipaddress.IPv6Address) -> bool:
        return self._dhcp_start <= address <= self._dhcp_end

    def _static_addresses(self) -> set[str]:
        return {str(host) for host in self._network.hosts() if not self._in_dhcp_range(host)}

    def _validate_static_address(self, address: str) -> str:
        try:
            parsed = ipaddress.ip_address(address)
        except ValueError as exc:
            raise ValueError(f"'{address}' is not a valid IP address") from exc
        if parsed not in self._network:
            raise ValueError(f"{address} is not inside {self._network}")
        if self._in_dhcp_range(parsed):
            raise ValueError(f"{address} is inside the DHCP range ({self._dhcp_start}-{self._dhcp_end}) and is not tracked by IPAM")
        return str(parsed)

    async def _guest_entries(self) -> dict[str, IpamEntry]:
        snapshot = await self._infrastructure.list_resources()
        guests = [resource for resource in snapshot.resources if resource.is_guest]
        results = await asyncio.gather(
            *(self._guest_address_provider.guest_static_address(guest) for guest in guests),
            return_exceptions=True,
        )
        entries: dict[str, IpamEntry] = {}
        for guest, result in zip(guests, results):
            if isinstance(result, BaseException) or not result:
                continue
            try:
                parsed = ipaddress.ip_address(result)
            except ValueError:
                continue
            if parsed not in self._network or self._in_dhcp_range(parsed):
                continue
            entries[str(parsed)] = IpamEntry(address=str(parsed), source="guest", label=guest.name, resource_id=guest.id)
        return entries

    async def snapshot(self) -> IpamSnapshot:
        guest_entries = await self._guest_entries()
        manual_entries = {entry.address: entry for entry in self._repository.list_entries()}

        used: dict[str, IpamEntry] = dict(guest_entries)
        conflicts: list[IpamConflict] = []
        for address, manual_entry in manual_entries.items():
            guest_entry = guest_entries.get(address)
            if guest_entry is not None:
                conflicts.append(IpamConflict(address=address, guest_entry=guest_entry, manual_entry=manual_entry))
                continue
            used[address] = manual_entry

        free = sorted(self._static_addresses() - used.keys(), key=lambda addr: tuple(int(part) for part in addr.split(".")))
        used_sorted = sorted(used.values(), key=lambda entry: tuple(int(part) for part in entry.address.split(".")))
        return IpamSnapshot(cidr=str(self._network), dhcp_range=(str(self._dhcp_start), str(self._dhcp_end)), used=tuple(used_sorted), free=tuple(free), conflicts=tuple(conflicts))

    async def add_manual_entry(self, *, address: str, label: str, notes: str = "") -> IpamEntry:
        validated = self._validate_static_address(address)
        label = label.strip()
        if not label:
            raise ValueError("label is required")
        guest_entries = await self._guest_entries()
        existing_guest = guest_entries.get(validated)
        if existing_guest is not None:
            raise ValueError(f"{validated} is already in use by guest '{existing_guest.label}'")
        entry = IpamEntry(address=validated, source="manual", label=label, notes=notes.strip())
        self._repository.upsert_entry(entry)
        return entry

    def delete_manual_entry(self, address: str) -> None:
        existing = self._repository.get_entry(address)
        if existing is None:
            raise KeyError(address)
        if existing.source != "manual":
            raise ValueError(f"{address} is not a manually registered entry")
        self._repository.delete_entry(address)
