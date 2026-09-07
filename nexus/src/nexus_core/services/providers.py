import asyncio

from nexus_core.ports.providers import InfrastructureProvider, ProviderStatus


class ProviderService:
    def __init__(self, providers: dict[str, InfrastructureProvider]) -> None:
        self._providers = providers

    def names(self) -> tuple[str, ...]:
        return tuple(sorted(self._providers))

    async def health(self, provider_name: str) -> ProviderStatus:
        try:
            provider = self._providers[provider_name]
        except KeyError as exc:
            raise ValueError(f"unknown provider: {provider_name}") from exc
        return await provider.health()

    async def health_all(self) -> list[ProviderStatus]:
        names = self.names()
        return list(await asyncio.gather(*(self.health(name) for name in names)))
