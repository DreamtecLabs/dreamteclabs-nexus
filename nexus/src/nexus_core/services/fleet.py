from __future__ import annotations

import asyncio

from nexus_core.ports.fleet import FleetAddressProvider, FleetRunResult, FleetScript, FleetScriptRepository, FleetTarget
from nexus_core.services.ssh_bootstrap import BootstrapResult, SshBootstrapService


class FleetOperationsDisabled(Exception):
    pass


class FleetService:
    """Ad-hoc "push this to the whole fleet" operations for guests Nexus didn't
    necessarily provision itself -- a lightweight, script-library alternative to
    running Ansible by hand. Scripts are discovered from disk (FleetScriptRepository)
    so adding one is a file drop, not a Nexus Core change or deploy."""

    def __init__(
        self,
        script_repository: FleetScriptRepository,
        infrastructure_service,
        address_provider: FleetAddressProvider,
        ssh_bootstrap: SshBootstrapService,
        *,
        enabled: bool = False,
    ) -> None:
        self._scripts = script_repository
        self._infrastructure = infrastructure_service
        self._addresses = address_provider
        self._ssh_bootstrap = ssh_bootstrap
        self.enabled = enabled

    def list_scripts(self) -> list[FleetScript]:
        return self._scripts.list_scripts()

    async def list_targets(self) -> list[FleetTarget]:
        snapshot = await self._infrastructure.list_resources()
        guests = [resource for resource in snapshot.resources if resource.type == "pve-lxc" and resource.status == "running"]
        addresses = await asyncio.gather(
            *(self._addresses.guest_static_address(guest) for guest in guests),
            return_exceptions=True,
        )
        return [
            FleetTarget(resource_id=guest.id, name=guest.name, address=None if isinstance(address, BaseException) else address)
            for guest, address in zip(guests, addresses)
        ]

    async def enroll_key(self, *, address: str, password: str, username: str = "root") -> BootstrapResult:
        if not self.enabled:
            raise FleetOperationsDisabled("Fleet operations are disabled by configuration")
        return await self._ssh_bootstrap.enroll_key(address, username=username, password=password)

    async def run(self, *, script: str, targets: list[dict[str, str]], params: dict[str, str]) -> list[FleetRunResult]:
        if not self.enabled:
            raise FleetOperationsDisabled("Fleet operations are disabled by configuration")
        body = self._scripts.read_script(script)

        async def _run_one(target: dict[str, str]) -> FleetRunResult:
            result = await self._ssh_bootstrap.wait_and_run(target["address"], username="root", script=body, env=params)
            return FleetRunResult(resource_id=target["resource_id"], name=target["name"], address=target["address"], ok=result.ok, detail=result.detail)

        return list(await asyncio.gather(*(_run_one(target) for target in targets)))
