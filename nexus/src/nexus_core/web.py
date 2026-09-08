from pathlib import Path

from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates

from nexus_core.services.infrastructure import InfrastructureResourceNotFound

_PACKAGE_ROOT = Path(__file__).resolve().parent
_templates = Jinja2Templates(directory=str(_PACKAGE_ROOT / "templates"))
router = APIRouter(include_in_schema=False)


def _human_bytes(value: int | None) -> str:
    if value is None:
        return "—"
    amount = float(value)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB", "PiB"):
        if amount < 1024 or unit == "PiB":
            return f"{amount:.1f} {unit}" if unit not in {"B", "KiB"} else f"{amount:.0f} {unit}"
        amount /= 1024
    return "—"


def _human_uptime(value: int | None) -> str:
    if value is None:
        return "—"
    days, remainder = divmod(value, 86400)
    hours, remainder = divmod(remainder, 3600)
    minutes = remainder // 60
    if days:
        return f"{days}d {hours}h"
    if hours:
        return f"{hours}h {minutes}m"
    return f"{minutes}m"


def _pct(used: float | int | None, total: float | int | None = None) -> str:
    if used is None:
        return "—"
    ratio = float(used) if total in {None, 0} else float(used) / float(total)
    return f"{ratio * 100:.1f}%"


def _resource_view(resource):
    return {
        "resource": resource,
        "cpu": _pct(resource.cpu_usage),
        "memory": f"{_human_bytes(resource.memory_used_bytes)} / {_human_bytes(resource.memory_total_bytes)}",
        "memory_pct": _pct(resource.memory_used_bytes, resource.memory_total_bytes),
        "disk": f"{_human_bytes(resource.disk_used_bytes)} / {_human_bytes(resource.disk_total_bytes)}",
        "disk_pct": _pct(resource.disk_used_bytes, resource.disk_total_bytes),
        "uptime": _human_uptime(resource.uptime_seconds),
    }


@router.get("/", response_class=HTMLResponse)
async def index(request: Request) -> HTMLResponse:
    providers = []
    for name in request.app.state.provider_service.names():
        status = await request.app.state.provider_service.health(name)
        providers.append({"name": name, "healthy": status.healthy, "detail": status.detail})
    targets = request.app.state.monitoring_service.list_targets()
    return _templates.TemplateResponse(request=request, name="index.html", context={"providers": providers, "monitoring_count": len(targets)})


async def _infrastructure_snapshot(request: Request):
    try:
        return await request.app.state.infrastructure_service.list_resources()
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc


@router.get("/infrastructure", response_class=HTMLResponse)
async def infrastructure(request: Request) -> HTMLResponse:
    snapshot = await _infrastructure_snapshot(request)
    resources = list(snapshot.resources)
    guests = [resource for resource in resources if resource.type in {"pve-lxc", "pve-qemu"}]
    summary = {"total": len(resources), "guests": len(guests), "running": sum(resource.status == "running" for resource in guests), "stopped": sum(resource.status == "stopped" for resource in guests), "nodes": sum(resource.type == "pve-node" for resource in resources), "pbs": sum(resource.type in {"pbs-node", "pbs-datastore"} for resource in resources)}
    return _templates.TemplateResponse(request=request, name="infrastructure.html", context={"resources": resources, "remote_errors": snapshot.remote_errors, "summary": summary})


@router.get("/infrastructure/estate", response_class=HTMLResponse)
async def estate(request: Request) -> HTMLResponse:
    try:
        remotes = await request.app.state.infrastructure_service.estate_summary()
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    totals = {"remotes": len(remotes), "resources": sum(item.resources for item in remotes), "guests": sum(item.guests for item in remotes), "running": sum(item.running for item in remotes), "nodes": sum(item.pve_nodes for item in remotes)}
    return _templates.TemplateResponse(request=request, name="estate.html", context={"remotes": remotes, "totals": totals, "human_bytes": _human_bytes, "pct": _pct, "human_uptime": _human_uptime})


@router.get("/infrastructure/resource", response_class=HTMLResponse)
async def resource_detail(request: Request, id: str) -> HTMLResponse:
    try:
        context = await request.app.state.infrastructure_service.get_resource_context(id)
    except InfrastructureResourceNotFound as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return _templates.TemplateResponse(request=request, name="resource.html", context={"context": context, "view": _resource_view(context.resource), "human_bytes": _human_bytes, "pct": _pct, "human_uptime": _human_uptime})


@router.get("/infrastructure/power", response_class=HTMLResponse)
async def power_center(request: Request) -> HTMLResponse:
    snapshot = await _infrastructure_snapshot(request)
    service = request.app.state.infrastructure_service
    guests = [resource for resource in snapshot.resources if resource.type in {"pve-lxc", "pve-qemu"}]
    rows = [{"resource": resource, "actions": service.available_power_actions(resource)} for resource in guests]
    summary = {"guests": len(guests), "running": sum(resource.status == "running" for resource in guests), "stopped": sum(resource.status == "stopped" for resource in guests)}
    return _templates.TemplateResponse(request=request, name="power.html", context={"rows": rows, "remote_errors": snapshot.remote_errors, "summary": summary, "power_enabled": service.power_operations_enabled, "history": service.list_recent_power_operations(10)})


@router.get("/monitoring", response_class=HTMLResponse)
async def monitoring(request: Request) -> HTMLResponse:
    service = request.app.state.monitoring_service
    targets = service.list_targets()
    statuses = await service.list_statuses()
    status_by_id = {status.target_id: status for status in statuses}
    rows = [{"target": target, "status": status_by_id.get(target.id)} for target in targets]
    return _templates.TemplateResponse(request=request, name="monitoring.html", context={"rows": rows, "summary": service.summarize(statuses), "provider": await service.provider_diagnostics()})
