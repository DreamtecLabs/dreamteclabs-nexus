from contextlib import asynccontextmanager
from dataclasses import asdict
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from nexus_core.config import Settings, get_settings
from nexus_core.provisioning_api import install_provisioning
from nexus_core.providers.alerting import UnconfiguredAlertingProvider, UnconfiguredMetricsProvider
from nexus_core.providers.cloudflare import CloudflareApiProvider, UnconfiguredCloudflareProvider
from nexus_core.providers.domain_diagnostics import PublicDomainDiagnosticsProvider
from nexus_core.providers.domain_helper import DomainHelperProvider
from nexus_core.providers.file_sd import CompositeTelemetryRuntime, FileSdTelemetryRuntime, IcmpFileSdTelemetryRuntime
from nexus_core.providers.pdm import PdmProvider
from nexus_core.providers.signoz import SigNozAlertingProvider, SigNozMetricsProvider
from nexus_core.ports.domains import TunnelIngressRule
from nexus_core.repositories.domain_audit_jsonl import JsonlDomainAuditRepository
from nexus_core.repositories.domains_json import JsonDomainRepository
from nexus_core.repositories.ipam_json import JsonIpamRepository
from nexus_core.repositories.monitoring_json import JsonMonitoringRepository
from nexus_core.repositories.power_audit_jsonl import JsonlPowerAuditRepository
from nexus_core.services.cloudflare import CloudflareOperationsDisabled, CloudflareService
from nexus_core.services.domains import DomainOperationsDisabled, DomainService, DomainVerificationFailed
from nexus_core.services.infrastructure import DecommissionDisabled, DecommissionNotAllowed, DecommissionVerificationTimeout, InfrastructureResourceNotFound, InfrastructureService, PowerActionNotAllowed, PowerOperationsDisabled, PowerVerificationTimeout
from nexus_core.services.ipam import IpamService
from nexus_core.services.monitoring import MonitoringService
from nexus_core.services.providers import ProviderService
from nexus_core.web import router as web_router


class MonitoringTargetInput(BaseModel):
    name: str = Field(min_length=1, max_length=80)
    address: str = Field(min_length=1, max_length=253)
    site: str = Field(min_length=1, max_length=64)
    state: str = "enabled"
    profile: str = Field(default="prometheus", pattern="^(prometheus|icmp)$")
    port: int | None = Field(default=None, ge=1, le=65535)
    metrics_path: str | None = Field(default=None, max_length=128)
    health_metric: str = Field(default="up", min_length=1, max_length=128)


class MonitoringStateInput(BaseModel):
    state: str = Field(min_length=1, max_length=16)


class MonitoringHostMaintenanceInput(BaseModel):
    maintenance: bool


class PowerActionInput(BaseModel):
    resource_id: str = Field(min_length=1, max_length=255)
    action: str = Field(min_length=1, max_length=16)
    confirmation: str | None = Field(default=None, max_length=128)


class DecommissionInput(BaseModel):
    resource_id: str = Field(min_length=1, max_length=255)
    confirmation: str = Field(min_length=1, max_length=128)
    purge: bool = True


class IpamEntryInput(BaseModel):
    address: str = Field(min_length=1, max_length=45)
    label: str = Field(min_length=1, max_length=80)
    notes: str = Field(default="", max_length=500)


class DomainValidateInput(BaseModel):
    domain: str = Field(min_length=3, max_length=253)


class DomainReconcileInput(BaseModel):
    domain: str = Field(min_length=3, max_length=253)
    hestia_user: str = Field(default="admin", min_length=1, max_length=64)
    migrate: bool = False


class DnsRecordInput(BaseModel):
    type: str = Field(min_length=1, max_length=16)
    name: str = Field(min_length=1, max_length=253)
    content: str = Field(default="", max_length=2048)
    ttl: int = Field(default=1, ge=1, le=86400)
    proxied: bool = False
    priority: int | None = Field(default=None, ge=0, le=65535)
    data: dict[str, object] | None = Field(default=None)


class TunnelIngressRuleInput(BaseModel):
    hostname: str | None = Field(default=None, max_length=253)
    service: str = Field(min_length=1, max_length=512)
    path: str | None = Field(default=None, max_length=512)
    no_tls_verify: bool = False
    http_host_header: str | None = Field(default=None, max_length=253)
    origin_server_name: str | None = Field(default=None, max_length=253)
    connect_timeout_seconds: int | None = Field(default=None, ge=1, le=300)


class TunnelIngressInput(BaseModel):
    rules: list[TunnelIngressRuleInput] = Field(min_length=1, max_length=200)


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    pdm = PdmProvider(base_url=settings.pdm_base_url, verify_tls=settings.pdm_verify_tls, health_path=settings.pdm_health_path, timeout_seconds=settings.provider_timeout_seconds, api_token_id=settings.pdm_api_token_id, api_token_secret=settings.pdm_api_token_secret)
    monitoring_repository = JsonMonitoringRepository(settings.monitoring_inventory_path)
    telemetry_runtime = CompositeTelemetryRuntime(
        [
            FileSdTelemetryRuntime(settings.monitoring_file_sd_path),
            IcmpFileSdTelemetryRuntime(settings.monitoring_icmp_file_sd_path),
        ]
    )
    power_audit_repository = JsonlPowerAuditRepository(settings.power_audit_path)
    domain_repository = JsonDomainRepository(settings.domains_inventory_path)
    domain_audit = JsonlDomainAuditRepository(settings.domains_audit_path)
    domain_diagnostics = PublicDomainDiagnosticsProvider(settings.provider_timeout_seconds)
    domain_orchestrator = DomainHelperProvider(settings.domains_helper_path, settings.domains_helper_timeout_seconds)
    if settings.signoz_api_key:
        alerting = SigNozAlertingProvider(base_url=settings.signoz_url, api_key=settings.signoz_api_key, timeout_seconds=settings.provider_timeout_seconds)
        metrics = SigNozMetricsProvider(base_url=settings.signoz_url, api_key=settings.signoz_api_key, timeout_seconds=settings.provider_timeout_seconds)
    else:
        alerting = UnconfiguredAlertingProvider()
        metrics = UnconfiguredMetricsProvider()
    if settings.cloudflare_api_token and settings.cloudflare_account_id and settings.cloudflare_tunnel_id:
        cloudflare = CloudflareApiProvider(
            api_base=settings.cloudflare_api_base,
            api_token=settings.cloudflare_api_token,
            account_id=settings.cloudflare_account_id,
            tunnel_id=settings.cloudflare_tunnel_id,
            timeout_seconds=settings.provider_timeout_seconds,
        )
    else:
        cloudflare = UnconfiguredCloudflareProvider()

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        await telemetry_runtime.reconcile(monitoring_repository.list_targets())
        yield

    app = FastAPI(title="DreamtecLabs Nexus", version="0.9.0", lifespan=lifespan)
    app.state.provider_service = ProviderService({pdm.name: pdm})
    app.state.monitoring_service = MonitoringService(monitoring_repository, alerting, telemetry_runtime, metrics)
    app.state.infrastructure_service = InfrastructureService(pdm, power_operations_enabled=settings.power_operations_enabled, verification_attempts=settings.power_verification_attempts, verification_interval_seconds=settings.power_verification_interval_seconds, audit_repository=power_audit_repository, decommission_enabled=settings.decommission_enabled, monitoring_service=app.state.monitoring_service)
    app.state.ipam_service = IpamService(JsonIpamRepository(settings.ipam_manual_path), app.state.infrastructure_service, pdm, cidr=settings.ipam_cidr, dhcp_range_start=settings.ipam_dhcp_range_start, dhcp_range_end=settings.ipam_dhcp_range_end)
    app.state.domain_service = DomainService(domain_repository, domain_diagnostics, domain_orchestrator, domain_audit, operations_enabled=settings.domains_operations_enabled, verification_attempts=settings.domains_verification_attempts, verification_interval_seconds=settings.domains_verification_interval_seconds)
    app.state.cloudflare_service = CloudflareService(cloudflare, domain_audit, operations_enabled=settings.domains_operations_enabled)
    install_provisioning(app, settings, app.state.infrastructure_service, app.state.monitoring_service)
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
        return {"resources": [asdict(resource) for resource in snapshot.resources], "remote_errors": [asdict(error) for error in snapshot.remote_errors], "count": len(snapshot.resources), "power_operations_enabled": request.app.state.infrastructure_service.power_operations_enabled, "decommission_enabled": request.app.state.infrastructure_service.decommission_enabled}

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

    @app.post("/api/v1/infrastructure/decommission")
    async def infrastructure_decommission(payload: DecommissionInput, request: Request) -> dict[str, object]:
        try:
            result = await request.app.state.infrastructure_service.decommission(**payload.model_dump())
        except DecommissionDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except InfrastructureResourceNotFound as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except DecommissionNotAllowed as exc:
            raise HTTPException(status_code=409, detail=str(exc)) from exc
        except DecommissionVerificationTimeout as exc:
            raise HTTPException(status_code=504, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(result)

    @app.get("/api/v1/ipam")
    async def ipam_snapshot(request: Request) -> dict[str, object]:
        snapshot = await request.app.state.ipam_service.snapshot()
        return asdict(snapshot)

    @app.post("/api/v1/ipam/entries")
    async def ipam_add_entry(payload: IpamEntryInput, request: Request) -> dict[str, object]:
        try:
            entry = await request.app.state.ipam_service.add_manual_entry(**payload.model_dump())
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        return asdict(entry)

    @app.delete("/api/v1/ipam/entries/{address}")
    async def ipam_delete_entry(address: str, request: Request) -> dict[str, str]:
        try:
            request.app.state.ipam_service.delete_manual_entry(address)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        return {"address": address, "status": "deleted"}

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

    @app.patch("/api/v1/monitoring/hosts/{host_name}/maintenance")
    async def set_monitoring_host_maintenance(host_name: str, payload: MonitoringHostMaintenanceInput, request: Request) -> dict[str, object]:
        try:
            await request.app.state.monitoring_service.set_host_maintenance(host_name, payload.maintenance)
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"host": host_name, "maintenance": payload.maintenance}

    @app.delete("/api/v1/monitoring/targets/{target_id}")
    async def delete_monitoring_target(target_id: str, request: Request) -> dict[str, object]:
        try:
            target = await request.app.state.monitoring_service.delete_target(target_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=f"monitoring target '{target_id}' was not found") from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(target)

    @app.get("/api/v1/domains")
    async def domains(request: Request) -> dict[str, object]:
        service = request.app.state.domain_service
        items = service.list_domains()
        return {"domains": [asdict(item) for item in items], "count": len(items), "operations_enabled": service.operations_enabled}

    @app.post("/api/v1/domains/validate")
    async def validate_domain(payload: DomainValidateInput, request: Request) -> dict[str, object]:
        try:
            validation = await request.app.state.domain_service.validate(payload.domain)
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(validation)

    @app.get("/api/v1/domains/audit")
    async def domains_audit(request: Request, limit: int = 25) -> dict[str, object]:
        entries = request.app.state.domain_service.list_recent_operations(limit)
        return {"operations": [asdict(item) for item in entries], "count": len(entries)}

    @app.post("/api/v1/domains/reconcile")
    async def reconcile_domain(payload: DomainReconcileInput, request: Request) -> dict[str, object]:
        try:
            result, validation = await request.app.state.domain_service.reconcile(name=payload.domain, hestia_user=payload.hestia_user, migrate=payload.migrate)
        except DomainOperationsDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except DomainVerificationFailed as exc:
            raise HTTPException(status_code=502, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"operation": asdict(result), "validation": asdict(validation)}

    @app.get("/api/v1/cloudflare/zones/{zone_name}/dns-records")
    async def list_dns_records(zone_name: str, request: Request) -> dict[str, object]:
        try:
            records = await request.app.state.cloudflare_service.list_dns_records(zone_name)
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"records": [asdict(record) for record in records], "count": len(records)}

    @app.post("/api/v1/cloudflare/zones/{zone_name}/dns-records")
    async def create_dns_record(zone_name: str, payload: DnsRecordInput, request: Request) -> dict[str, object]:
        try:
            record = await request.app.state.cloudflare_service.create_dns_record(zone_name, **payload.model_dump())
        except CloudflareOperationsDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(record)

    @app.put("/api/v1/cloudflare/zones/{zone_name}/dns-records/{record_id}")
    async def update_dns_record(zone_name: str, record_id: str, payload: DnsRecordInput, request: Request) -> dict[str, object]:
        try:
            record = await request.app.state.cloudflare_service.update_dns_record(zone_name, record_id, **payload.model_dump())
        except CloudflareOperationsDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return asdict(record)

    @app.delete("/api/v1/cloudflare/zones/{zone_name}/dns-records/{record_id}")
    async def delete_dns_record(zone_name: str, record_id: str, request: Request) -> dict[str, object]:
        try:
            await request.app.state.cloudflare_service.delete_dns_record(zone_name, record_id)
        except CloudflareOperationsDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"zone": zone_name, "id": record_id, "deleted": True}

    @app.get("/api/v1/cloudflare/tunnel/ingress")
    async def tunnel_ingress(request: Request) -> dict[str, object]:
        try:
            rules = await request.app.state.cloudflare_service.list_tunnel_ingress()
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"rules": [asdict(rule) for rule in rules], "count": len(rules)}

    @app.put("/api/v1/cloudflare/tunnel/ingress")
    async def set_tunnel_ingress(payload: TunnelIngressInput, request: Request) -> dict[str, object]:
        rules = [TunnelIngressRule(**rule.model_dump()) for rule in payload.rules]
        try:
            saved = await request.app.state.cloudflare_service.set_tunnel_ingress(rules)
        except CloudflareOperationsDisabled as exc:
            raise HTTPException(status_code=403, detail=str(exc)) from exc
        except ValueError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        return {"rules": [asdict(rule) for rule in saved], "count": len(saved)}

    return app


app = create_app()
