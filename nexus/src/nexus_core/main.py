from contextlib import asynccontextmanager
from dataclasses import asdict
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from nexus_core.config import Settings, get_settings
from nexus_core.providers.alerting import UnconfiguredAlertingProvider
from nexus_core.providers.file_sd import FileSdTelemetryRuntime
from nexus_core.providers.pdm import PdmProvider
from nexus_core.providers.signoz import SigNozAlertingProvider
from nexus_core.repositories.monitoring_json import JsonMonitoringRepository
from nexus_core.services.monitoring import MonitoringService
from nexus_core.services.providers import ProviderService
from nexus_core.web import router as web_router


class MonitoringTargetInput(BaseModel):
    name: str = Field(min_length=1, max_length=80)
    address: str = Field(min_length=1, max_length=253)
    port: int = Field(ge=1, le=65535)
    metrics_path: str = Field(default="/metrics", max_length=128)
    site: str = Field(min_length=1, max_length=64)
    state: str = "enabled"


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    pdm = PdmProvider(
        base_url=settings.pdm_base_url,
        verify_tls=settings.pdm_verify_tls,
        health_path=settings.pdm_health_path,
        timeout_seconds=settings.provider_timeout_seconds,
        api_token_id=settings.pdm_api_token_id,
        api_token_secret=settings.pdm_api_token_secret,
    )

    monitoring_repository = JsonMonitoringRepository(settings.monitoring_inventory_path)
    telemetry_runtime = FileSdTelemetryRuntime(settings.monitoring_file_sd_path)
    if settings.signoz_api_key:
        alerting = SigNozAlertingProvider(
            base_url=settings.signoz_url,
            api_key=settings.signoz_api_key,
            timeout_seconds=settings.provider_timeout_seconds,
        )
    else:
        alerting = UnconfiguredAlertingProvider()

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        await telemetry_runtime.reconcile(monitoring_repository.list_targets())
        yield

    app = FastAPI(title="DreamtecLabs Nexus", version="0.3.1", lifespan=lifespan)
    app.state.provider_service = ProviderService({pdm.name: pdm})
    app.state.monitoring_service = MonitoringService(monitoring_repository, alerting, telemetry_runtime)

    static_dir = Path(__file__).resolve().parent / "static"
    app.mount("/static", StaticFiles(directory=str(static_dir)), name="static")
    app.include_router(web_router)

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

    @app.get("/api/v1/monitoring/targets")
    async def monitoring_targets(request: Request) -> dict[str, object]:
        targets = request.app.state.monitoring_service.list_targets()
        return {"targets": [asdict(target) for target in targets], "count": len(targets)}

    @app.put("/api/v1/monitoring/targets")
    async def upsert_monitoring_target(payload: MonitoringTargetInput, request: Request) -> dict[str, object]:
        try:
            target = await request.app.state.monitoring_service.upsert_target(**payload.model_dump())
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(target)

    @app.delete("/api/v1/monitoring/targets/{target_id}")
    async def delete_monitoring_target(target_id: str, request: Request) -> dict[str, object]:
        try:
            target = await request.app.state.monitoring_service.delete_target(target_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=f"monitoring target '{target_id}' was not found") from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(target)

    return app


app = create_app()
