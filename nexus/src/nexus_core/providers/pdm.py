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
        api_token_id: str | None = None,
        api_token_secret: str | None = None,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._verify_tls = verify_tls
        self._health_path = "/" + health_path.lstrip("/")
        self._timeout_seconds = timeout_seconds
        self._api_token_id = api_token_id
        self._api_token_secret = api_token_secret
        self._transport = transport

    def _headers(self) -> dict[str, str]:
        if self._api_token_id and self._api_token_secret:
            return {
                "Authorization": f"PDMAPIToken {self._api_token_id}:{self._api_token_secret}",
            }
        return {}

    async def health(self) -> ProviderStatus:
        try:
            async with httpx.AsyncClient(
                base_url=self._base_url,
                verify=self._verify_tls,
                timeout=self._timeout_seconds,
                transport=self._transport,
                headers=self._headers(),
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
