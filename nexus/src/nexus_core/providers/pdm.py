import httpx

from nexus_core.ports.providers import ProviderStatus


class PdmProvider:
    name = "pdm"

    def __init__(
        self,
        *,
        base_url: str,
        verify_tls: bool,
        health_path: str,
        timeout_seconds: float,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._verify_tls = verify_tls
        self._health_path = "/" + health_path.lstrip("/")
        self._timeout_seconds = timeout_seconds
        self._transport = transport

    async def health(self) -> ProviderStatus:
        try:
            async with httpx.AsyncClient(
                base_url=self._base_url,
                verify=self._verify_tls,
                timeout=self._timeout_seconds,
                transport=self._transport,
            ) as client:
                response = await client.get(self._health_path)
                response.raise_for_status()
        except httpx.HTTPStatusError as exc:
            return ProviderStatus(
                provider=self.name,
                healthy=False,
                detail=f"HTTP {exc.response.status_code} on {self._health_path}",
            )
        except httpx.HTTPError as exc:
            return ProviderStatus(provider=self.name, healthy=False, detail=type(exc).__name__)

        return ProviderStatus(provider=self.name, healthy=True)
