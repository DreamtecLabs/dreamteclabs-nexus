from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True, slots=True)
class ProviderStatus:
    provider: str
    healthy: bool
    detail: str | None = None


class InfrastructureProvider(Protocol):
    name: str

    async def health(self) -> ProviderStatus: ...
