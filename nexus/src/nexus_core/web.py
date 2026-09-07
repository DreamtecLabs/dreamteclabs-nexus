from pathlib import Path

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates

_PACKAGE_ROOT = Path(__file__).resolve().parent
_templates = Jinja2Templates(directory=str(_PACKAGE_ROOT / "templates"))

router = APIRouter(include_in_schema=False)


@router.get("/", response_class=HTMLResponse)
async def index(request: Request) -> HTMLResponse:
    providers = []
    for name in request.app.state.provider_service.names():
        status = await request.app.state.provider_service.health(name)
        providers.append({"name": name, "healthy": status.healthy, "detail": status.detail})
    targets = request.app.state.monitoring_service.list_targets()
    return _templates.TemplateResponse(request=request, name="index.html", context={"providers": providers, "monitoring_count": len(targets)})


@router.get("/infrastructure", response_class=HTMLResponse)
async def infrastructure(request: Request) -> HTMLResponse:
    snapshot = await request.app.state.infrastructure_service.list_resources()
    resources = list(snapshot.resources)
    guest_types = {"lxc", "qemu", "pve-lxc", "pve-qemu"}
    node_types = {"node", "pve-node"}
    guests = [resource for resource in resources if resource.type in guest_types]
    summary = {
        "total": len(resources),
        "guests": len(guests),
        "running": sum(resource.status == "running" for resource in guests),
        "stopped": sum(resource.status == "stopped" for resource in guests),
        "nodes": sum(resource.type in node_types for resource in resources),
        "pbs": sum(resource.type in {"pbs-node", "pbs-datastore"} for resource in resources),
    }
    return _templates.TemplateResponse(
        request=request,
        name="infrastructure.html",
        context={"resources": resources, "remote_errors": snapshot.remote_errors, "summary": summary},
    )


@router.get("/monitoring", response_class=HTMLResponse)
async def monitoring(request: Request) -> HTMLResponse:
    targets = request.app.state.monitoring_service.list_targets()
    return _templates.TemplateResponse(request=request, name="monitoring.html", context={"targets": targets})
