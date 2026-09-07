import httpx
import pytest

from nexus_core.providers.pdm import PdmProvider


@pytest.mark.asyncio
async def test_pdm_provider_reports_healthy() -> None:
    async def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api2/json/version"
        return httpx.Response(200, json={"data": {"version": "test"}})

    provider = PdmProvider(
        base_url="https://pdm.test",
        verify_tls=True,
        health_path="/api2/json/version",
        timeout_seconds=1,
        transport=httpx.MockTransport(handler),
    )

    status = await provider.health()

    assert status.provider == "pdm"
    assert status.healthy is True
    assert status.detail is None


@pytest.mark.asyncio
async def test_pdm_provider_sends_api_token_authorization_header() -> None:
    async def handler(request: httpx.Request) -> httpx.Response:
        assert request.headers["Authorization"] == "PDMAPIToken nexus@pdm!nexus-core:secret-token"
        return httpx.Response(200, json={"data": {"version": "test"}})

    provider = PdmProvider(
        base_url="https://pdm.test",
        verify_tls=True,
        health_path="/api2/json/version",
        timeout_seconds=1,
        api_token_id="nexus@pdm!nexus-core",
        api_token_secret="secret-token",
        transport=httpx.MockTransport(handler),
    )

    status = await provider.health()

    assert status.healthy is True


@pytest.mark.asyncio
async def test_pdm_provider_does_not_send_partial_credentials() -> None:
    async def handler(request: httpx.Request) -> httpx.Response:
        assert "Authorization" not in request.headers
        return httpx.Response(401)

    provider = PdmProvider(
        base_url="https://pdm.test",
        verify_tls=True,
        health_path="/api2/json/version",
        timeout_seconds=1,
        api_token_id="nexus@pdm!nexus-core",
        transport=httpx.MockTransport(handler),
    )

    status = await provider.health()

    assert status.healthy is False
    assert status.detail == "HTTP 401 on /api2/json/version"


@pytest.mark.asyncio
async def test_pdm_provider_reports_safe_http_failure_detail() -> None:
    async def handler(_: httpx.Request) -> httpx.Response:
        return httpx.Response(503)

    provider = PdmProvider(
        base_url="https://pdm.test",
        verify_tls=True,
        health_path="/api2/json/version",
        timeout_seconds=1,
        transport=httpx.MockTransport(handler),
    )

    status = await provider.health()

    assert status.provider == "pdm"
    assert status.healthy is False
    assert status.detail == "HTTP 503 on /api2/json/version"
