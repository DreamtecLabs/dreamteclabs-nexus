from nexus_core.ports.providers import InfrastructureProvider, ProviderStatus


class ProviderService:
    def __init__(self, providers: dict[str, InfrastructureProvider]) -> None:
        self._providers = providers

    async def health(self, provider_name: str) -> ProviderStatus:
        try:
            provider = self._providers[provider_name]
        except KeyError as exc:
            raise ValueError(f"unknown provider: {provider_name}") from exc
        return await provider.health()
