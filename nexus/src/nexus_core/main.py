from contextlib import asynccontextmanager
from dataclasses import asdict
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from nexus_core.config import Settings, get_settings
from nexus_core.providers.alerting import UnconfiguredAlertingProvider, UnconfiguredMetricsProvider
from nexus_core.providers.file_sd import FileSdTelemetryRuntime
from nexus_core.providers.pdm import PdmProvider
from nexus_core.providers.signoz import SigNozAlertingProvider, SigNozMetricsProvider
from nexus_core.repositories.monitoring_json import JsonMonitoringRepository
from nexus_core.repositories.power_audit_jsonl import JsonlPowerAuditRepository
from nexus_core.services.infrastructure import (
    InfrastructureResourceNotFound,
    InfrastructureService,
    PowerActionNotAllowed,
    PowerOperationsDisabled,
    PowerVerificationTimeout,
)
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
    health_metric: str = Field(default="up", min_length=1, max_length=128)


class MonitoringStateInput(BaseModel):
    state: str = Field(min_length=1, max_length=16)


class PowerActionInput(BaseModel):
    resource_id: str = Field(min_length=1, max_length=255)
    action: str = Field(min_length=1, max_length=16)
    confirmation: str | None = Field(default=None, max_length=128)


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    pdm = PdmProvider(base_url=settings.pdm_base_url, verify_tls=settings.pdm_verify_tls, health_path=settings.pdm_health_path, timeout_seconds=settings.provider_timeout_seconds, api_token_id=settings.pdm_api_token_id, api_token_secret=settings.pdm_api_token_secret)
    monitoring_repository = JsonMonitoringRepository(settings.monitoring_inventory_path)
    telemetry_runtime = FileSdTelemetryRuntime(settings.monitoring_file_sd_path)
    power_audit_repository = JsonlPowerAuditRepository(settings.power_audit_path)
    if settings.signoz_api_key:
        alerting = SigNozAlertingProvider(base_url=settings.signoz_url, api_key=settings.signoz_api_key, timeout_seconds=settings.provider_timeout_seconds)
        metrics = SigNozMetricsProvider(base_url=settings.signoz_url, api_key=settings.signoz_api_key, timeout_seconds=settings.provider_timeout_seconds)
    else:
        alerting = UnconfiguredAlertingProvider()
        metrics = UnconfiguredMetricsProvider()

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        await telemetry_runtime.reconcile(monitoring_repository.list_targets())
        yield

    app = FastAPI(title="DreamtecLabs Nexus", version="0.7.0", lifespan=lifespan)
    app.state.provider_service = ProviderService({pdm.name: pdm})
    app.state.infrastructure_service = InfrastructureService(pdm, power_operations_enabled=settings.power_operations_enabled, verification_attempts=settings.power_verification_attempts, verification_interval_seconds=settings.power_verification_interval_seconds, audit_repository=power_audit_repository)
    app.state.monitoring_service = MonitoringService(monitoring_repository, alerting, telemetry_runtime, metrics)
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
        return {"provider": status.provider, "healthy": status.healthy, "detail": status.detail}

    @app.get("/api/v1/infrastructure/resources")
    async def infrastructure_resources(request: Request) -> dict[str, object]:
        try:
            snapshot = await request.app.state.infrastructure_service.list_resources()
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"resources": [asdict(resource) for resource in snapshot.resources], "remote_errors": [asdict(error) for error in snapshot.remote_errors], "count": len(snapshot.resources), "power_operations_enabled": request.app.state.infrastructure_service.power_operations_enabled}

    @app.get("/api/v1/infrastructure/resources/detail/{resource_id:path}")
    async def infrastructure_resource_detail(resource_id: str, request: Request) -> dict[str, object]:
        try:
            context = await request.app.state.infrastructure_service.get_resource_context(resource_id)
        except InfrastructureResourceNotFound as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(context)

    @app.get("/api/v1/infrastructure/estate")
    async def infrastructure_estate(request: Request) -> dict[str, object]:
        try:
            summaries = await request.app.state.infrastructure_service.estate_summary()
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"remotes": [asdict(item) for item in summaries], "count": len(summaries)}

    @app.get("/api/v1/infrastructure/power/history")
    async def infrastructure_power_history(request: Request, limit: int = 25) -> dict[str, object]:
        entries = request.app.state.infrastructure_service.list_recent_power_operations(limit)
        return {"operations": [asdict(entry) for entry in entries], "count": len(entries)}

    @app.post("/api/v1/infrastructure/power")
    async def infrastructure_power(payload: PowerActionInput, request: Request) -> dict[str, object]:
        try:
            result = await request.app.state.infrastructure_service.execute_power_action(**payload.model_dump())
        except PowerOperationsDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except InfrastructureResourceNotFound as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except PowerActionNotAllowed as exc:
            raise HTTPException(status_code=409, detail=str(exc)) from exc
        except PowerVerificationTimeout as exc:
            raise HTTPException(status_code=504, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(result)

    @app.get("/api/v1/monitoring/targets")
    async def monitoring_targets(request: Request) -> dict[str, object]:
        targets = request.app.state.monitoring_service.list_targets()
        return {"targets": [asdict(target) for target in targets], "count": len(targets)}

    @app.get("/api/v1/monitoring/status")
    async def monitoring_status(request: Request) -> dict[str, object]:
        service = request.app.state.monitoring_service
        statuses = await service.list_statuses()
        diagnostics = await service.provider_diagnostics()
        return {"statuses": [asdict(status) for status in statuses], "summary": service.summarize(statuses), "provider": asdict(diagnostics)}

    @app.get("/api/v1/monitoring/targets/{target_id}/status")
    async def monitoring_target_status(target_id: str, request: Request) -> dict[str, object]:
        service = request.app.state.monitoring_service
        target = next((item for item in service.list_targets() if item.id == target_id), None)
        if target is None:
            raise HTTPException(status_code=404, detail=f"monitoring target '{target_id}' was not found")
        return asdict(await service.get_status(target))

    @app.put("/api/v1/monitoring/targets")
    async def upsert_monitoring_target(payload: MonitoringTargetInput, request: Request) -> dict[str, object]:
        try:
            target = await request.app.state.monitoring_service.upsert_target(**payload.model_dump())
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(target)

    @app.patch("/api/v1/monitoring/targets/{target_id}/state")
    async def set_monitoring_target_state(target_id: str, payload: MonitoringStateInput, request: Request) -> dict[str, object]:
        try:
            target = await request.app.state.monitoring_service.set_state(target_id, payload.state)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=f"monitoring target '{target_id}' was not found") from exc
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
