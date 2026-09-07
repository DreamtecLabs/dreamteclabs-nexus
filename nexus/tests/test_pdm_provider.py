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
