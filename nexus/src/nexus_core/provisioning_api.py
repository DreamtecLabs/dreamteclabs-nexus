from __future__ import annotations

from dataclasses import asdict
from pathlib import Path

from fastapi import APIRouter, FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates
from pydantic import BaseModel, Field

from nexus_core.config import Settings
from nexus_core.ports.provisioning import GuestProvisionRequest
from nexus_core.providers.pdm_provisioning import PdmProvisioningProvider
from nexus_core.services.provisioning import (
    ProvisioningConflict,
    ProvisioningDisabled,
    ProvisioningService,
    ProvisioningVerificationTimeout,
)

_PACKAGE_ROOT = Path(__file__).resolve().parent
_templates = Jinja2Templates(directory=str(_PACKAGE_ROOT / "templates"))
router = APIRouter()


class ProvisionGuestInput(BaseModel):
    kind: str = Field(pattern="^(lxc|qemu)$")
    remote: str = Field(min_length=1, max_length=64)
    node: str = Field(min_length=1, max_length=64)
    vmid: int = Field(ge=100, le=999_999_999)
    name: str = Field(min_length=1, max_length=128)
    cores: int = Field(ge=1, le=1024)
    memory_mb: int = Field(ge=128)
    disk_gb: int = Field(ge=1)
    storage: str = Field(min_length=1, max_length=128)
    bridge: str = Field(min_length=1, max_length=64)
    source: str = Field(min_length=1, max_length=512)
    ip_config: str = Field(default="dhcp", max_length=128)
    gateway: str | None = Field(default=None, max_length=128)
    nameserver: str | None = Field(default=None, max_length=256)
    vlan: int | None = Field(default=None, ge=1, le=4094)
    onboot: bool = True
    start: bool = True
    unprivileged: bool = True
    nesting: bool = False
    ssh_enabled: bool = False
    ssh_public_key: str | None = Field(default=None, max_length=8192)
    root_password: str | None = Field(default=None, max_length=256)
    monitoring: str = Field(default="pdm", pattern="^(none|pdm|prometheus|icmp)$")
    advanced: dict[str, object] = Field(default_factory=dict)

    def to_request(self) -> GuestProvisionRequest:
        return GuestProvisionRequest(**self.model_dump())


_VMID_CHOICES_PER_REMOTE = 25
_VMID_SCAN_LIMIT = 2000


def _vmid_choices(options, remotes: list[str]) -> dict[str, list[int]]:
    choices: dict[str, list[int]] = {}
    for remote in remotes:
        used = set(options.used_vmids.get(remote, ()))
        candidate = options.next_vmids.get(remote, 100)
        found: list[int] = []
        scanned = 0
        while len(found) < _VMID_CHOICES_PER_REMOTE and scanned < _VMID_SCAN_LIMIT:
            if candidate not in used:
                found.append(candidate)
            candidate += 1
            scanned += 1
        choices[remote] = found
    return choices


@router.get("/provisioning", response_class=HTMLResponse, include_in_schema=False)
async def provisioning_page(request: Request) -> HTMLResponse:
    try:
        options = await request.app.state.provisioning_service.options()
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    remotes = sorted({item.remote for item in options.nodes}, key=str.casefold)
    default_remote = remotes[0] if remotes else None
    vmid_choices = _vmid_choices(options, remotes)
    default_vmid = vmid_choices.get(default_remote, [None])[0] if default_remote else None
    return _templates.TemplateResponse(
        request=request,
        name="provisioning.html",
        context={
            "options": options,
            "remotes": remotes,
            "default_vmid": default_vmid,
            "vmid_choices": vmid_choices,
            "enabled": request.app.state.provisioning_service.enabled,
        },
    )


@router.get("/api/v1/provisioning/storage-content")
async def provisioning_storage_content(remote: str, node: str, storage: str, content: str, request: Request) -> list[str]:
    try:
        volids = await request.app.state.provisioning_service.storage_content(remote, node, storage, content)
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return list(volids)


@router.get("/api/v1/provisioning/options")
async def provisioning_options(request: Request) -> dict[str, object]:
    try:
        options = await request.app.state.provisioning_service.options()
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    result = asdict(options)
    result["enabled"] = request.app.state.provisioning_service.enabled
    return result


@router.post("/api/v1/provisioning/plan")
async def provisioning_plan(payload: ProvisionGuestInput, request: Request) -> dict[str, object]:
    try:
        return await request.app.state.provisioning_service.plan(payload.to_request())
    except ProvisioningConflict as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc


@router.post("/api/v1/provisioning")
async def provision_guest(payload: ProvisionGuestInput, request: Request) -> dict[str, object]:
    try:
        result = await request.app.state.provisioning_service.provision(payload.to_request())
    except ProvisioningDisabled as exc:
        raise HTTPException(status_code=403, detail=str(exc)) from exc
    except ProvisioningConflict as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    except ProvisioningVerificationTimeout as exc:
        raise HTTPException(status_code=504, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return asdict(result)


def install_provisioning(
    app: FastAPI,
    settings: Settings,
    infrastructure_service,
    monitoring_service,
) -> None:
    provider = PdmProvisioningProvider(
        base_url=settings.pdm_base_url,
        verify_tls=settings.pdm_verify_tls,
        timeout_seconds=max(settings.provider_timeout_seconds, 30.0),
        api_token_id=settings.pdm_api_token_id,
        api_token_secret=settings.pdm_api_token_secret,
    )
    app.state.provisioning_service = ProvisioningService(
        provider,
        infrastructure_service,
        monitoring_service,
        enabled=settings.provisioning_enabled,
        verification_attempts=settings.provisioning_verification_attempts,
        verification_interval_seconds=settings.provisioning_verification_interval_seconds,
    )
    app.include_router(router)
