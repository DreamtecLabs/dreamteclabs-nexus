from __future__ import annotations

from nexus_core.ports.infrastructure import InfrastructureProvider, InfrastructureSnapshot


class InfrastructureService:
    def __init__(self, provider: InfrastructureProvider) -> None:
        self._provider = provider

    async def list_resources(self) -> InfrastructureSnapshot:
        return await self._provider.list_resources()
