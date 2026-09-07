from fastapi import FastAPI, HTTPException, Request

from nexus_core.config import Settings, get_settings
from nexus_core.providers.pdm import PdmProvider
from nexus_core.services.providers import ProviderService


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    pdm = PdmProvider(
        base_url=settings.pdm_base_url,
        verify_tls=settings.pdm_verify_tls,
        health_path=settings.pdm_health_path,
        timeout_seconds=settings.provider_timeout_seconds,
    )

    app = FastAPI(title="DreamtecLabs Nexus", version="0.1.0")
    app.state.provider_service = ProviderService({pdm.name: pdm})

    @app.get("/health")
    async def health() -> dict[str, str]:
        return {"status": "ok", "service": "nexus-core"}

    @app.get("/api/v1/providers/{provider_name}/health")
    async def provider_health(provider_name: str, request: Request) -> dict[str, object]:
        try:
            status = await request.app.state.provider_service.health(provider_name)
        except ValueError as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc

        return {
            "provider": status.provider,
            "healthy": status.healthy,
            "detail": status.detail,
        }

    return app


app = create_app()
