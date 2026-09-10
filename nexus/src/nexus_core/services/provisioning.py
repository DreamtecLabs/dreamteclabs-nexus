from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable

from nexus_core.ports.infrastructure import InfrastructureResource
from nexus_core.ports.provisioning import GuestProvisionRequest, ProvisioningOptions, ProvisioningProvider, ProvisioningResult, ProvisioningStep


class ProvisioningError(RuntimeError):
    pass


class ProvisioningDisabled(ProvisioningError):
    pass


class ProvisioningConflict(ProvisioningError):
    pass


class ProvisioningVerificationTimeout(ProvisioningError):
    pass


class ProvisioningService:
    def __init__(self, provider: ProvisioningProvider, infrastructure_service, monitoring_service, *, enabled: bool = False, verification_attempts: int = 60, verification_interval_seconds: float = 1.0, sleep: Callable[[float], Awaitable[None]] = asyncio.sleep) -> None:
        self._provider = provider
        self._infrastructure = infrastructure_service
        self._monitoring = monitoring_service
        self._enabled = enabled
        self._verification_attempts = max(1, verification_attempts)
        self._verification_interval_seconds = max(0.1, verification_interval_seconds)
        self._sleep = sleep

    @property
    def enabled(self) -> bool:
        return self._enabled

    async def options(self) -> ProvisioningOptions:
        return await self._provider.options()

    async def plan(self, request: GuestProvisionRequest) -> dict[str, object]:
        self._validate_request(request)
        options = await self.options()
        self._validate_selection(request, options)
        snapshot = await self._infrastructure.list_resources()
        self._ensure_available(request, snapshot.resources)
        return {"kind": request.kind, "remote": request.remote, "node": request.node, "vmid": request.vmid, "name": request.name, "cpu": f"{request.cores} cores", "memory_mb": request.memory_mb, "disk": f"{request.disk_gb} GiB on {request.storage}", "network": request.bridge, "ip": request.ip_config, "onboot": request.onboot, "start": request.start, "ssh": request.ssh_enabled, "monitoring": request.monitoring, "advanced_count": len(request.advanced)}

    async def provision(self, request: GuestProvisionRequest) -> ProvisioningResult:
        if not self._enabled:
            raise ProvisioningDisabled("Guest provisioning is disabled by configuration")
        await self.plan(request)
        steps: list[ProvisioningStep] = []
        warnings: list[str] = []
        try:
            created = await self._provider.create_guest(request)
            steps.append(ProvisioningStep("create", "success", "PDM accepted the PVE guest creation task"))
        except Exception as exc:
            steps.append(ProvisioningStep("create", "failed", str(exc)))
            raise
        resource: InfrastructureResource | None = None
        for attempt in range(self._verification_attempts):
            snapshot = await self._infrastructure.list_resources()
            resource = next((item for item in snapshot.resources if item.id == created.resource_id), None)
            if resource is not None and (not request.start or resource.status == "running"):
                break
            if attempt + 1 < self._verification_attempts:
                await self._sleep(self._verification_interval_seconds)
        else:
            raise ProvisioningVerificationTimeout(f"PDM accepted creation of {request.name}, but Nexus could not verify the expected resource state")
        assert resource is not None
        steps.append(ProvisioningStep("verify", "success", f"Read-back verified {resource.id} with state {resource.status}"))
        if request.ssh_enabled:
            if request.kind == "lxc" and request.ssh_public_key:
                steps.append(ProvisioningStep("ssh", "success", "SSH public key injected into the LXC creation request"))
            else:
                warnings.append("SSH was selected but no LXC public key was available for injection")
                steps.append(ProvisioningStep("ssh", "warning", warnings[-1]))
        else:
            steps.append(ProvisioningStep("ssh", "skipped", "SSH bootstrap not selected"))
        monitoring_mode = request.monitoring.strip().lower()
        if monitoring_mode == "none":
            steps.append(ProvisioningStep("monitoring", "skipped", "Monitoring not selected"))
        elif monitoring_mode == "pdm":
            steps.append(ProvisioningStep("monitoring", "success", "PDM-native state and resource telemetry are available immediately in Nexus"))
        elif monitoring_mode == "prometheus":
            address = self._guest_address(request)
            port = self._advanced_int(request.advanced.get("monitoring_port"), 9100)
            path = str(request.advanced.get("monitoring_path") or "/metrics")
            if not address:
                warning = "Prometheus monitoring requires a static IP/address; guest was created without target registration"
                warnings.append(warning)
                steps.append(ProvisioningStep("monitoring", "warning", warning))
            else:
                try:
                    await self._monitoring.upsert_target(name=request.name, address=address, profile="prometheus", port=port, metrics_path=path, site=str(request.advanced.get("monitoring_site") or "home"), state="enabled", health_metric=str(request.advanced.get("health_metric") or "up"))
                except Exception as exc:
                    warning = f"Guest was created, but monitoring registration failed ({type(exc).__name__})"
                    warnings.append(warning)
                    steps.append(ProvisioningStep("monitoring", "warning", warning))
                else:
                    steps.append(ProvisioningStep("monitoring", "success", f"Prometheus target registered at {address}:{port}{path}"))
        elif monitoring_mode == "icmp":
            address = self._guest_address(request)
            if not address:
                warning = "ICMP monitoring requires a static IP/address; guest was created without target registration"
                warnings.append(warning)
                steps.append(ProvisioningStep("monitoring", "warning", warning))
            else:
                try:
                    await self._monitoring.upsert_target(name=request.name, address=address, profile="icmp", site=str(request.advanced.get("monitoring_site") or "home"), state="enabled", health_metric="probe_success")
                except Exception as exc:
                    warning = f"Guest was created, but monitoring registration failed ({type(exc).__name__})"
                    warnings.append(warning)
                    steps.append(ProvisioningStep("monitoring", "warning", warning))
                else:
                    steps.append(ProvisioningStep("monitoring", "success", f"Agentless ICMP probe registered for {address}"))
        else:
            warning = f"Unknown monitoring mode '{request.monitoring}' was ignored"
            warnings.append(warning)
            steps.append(ProvisioningStep("monitoring", "warning", warning))
        result_status = "ready" if not warnings else "ready-with-warnings"
        return ProvisioningResult(resource_id=resource.id, resource_name=resource.name, status=result_status, task_reference=created.task_reference, steps=tuple(steps), warnings=tuple(warnings))

    @staticmethod
    def _validate_request(request: GuestProvisionRequest) -> None:
        if request.kind not in {"lxc", "qemu"}:
            raise ValueError("kind must be 'lxc' or 'qemu'")
        if not request.remote.strip() or not request.node.strip():
            raise ValueError("remote and node are required")
        if not request.name.strip():
            raise ValueError("guest name is required")
        if request.vmid < 100:
            raise ValueError("VMID must be at least 100")
        if request.cores < 1 or request.memory_mb < 128 or request.disk_gb < 1:
            raise ValueError("CPU, memory and disk values must be positive")
        if request.ssh_enabled and request.kind == "lxc" and not request.ssh_public_key:
            raise ValueError("SSH-enabled LXC provisioning requires a public key")

    @staticmethod
    def _validate_selection(request: GuestProvisionRequest, options: ProvisioningOptions) -> None:
        nodes = {(item.remote, item.name) for item in options.nodes}
        if (request.remote, request.node) not in nodes:
            raise ValueError(f"node '{request.node}' is not available on PDM remote '{request.remote}'")
        storages = {item.name for item in options.storages if item.remote == request.remote and item.node in {None, request.node}}
        if storages and request.storage not in storages:
            raise ValueError(f"storage '{request.storage}' is not available on {request.remote}/{request.node}")

    @staticmethod
    def _ensure_available(request: GuestProvisionRequest, resources: tuple[InfrastructureResource, ...]) -> None:
        for item in resources:
            if item.remote != request.remote:
                continue
            if item.vmid == request.vmid:
                raise ProvisioningConflict(f"VMID {request.vmid} already exists on remote '{request.remote}'")
            if item.type in {"pve-lxc", "pve-qemu"} and item.name.casefold() == request.name.casefold():
                raise ProvisioningConflict(f"guest name '{request.name}' already exists on remote '{request.remote}'")

    @staticmethod
    def _guest_address(request: GuestProvisionRequest) -> str | None:
        explicit = str(request.advanced.get("monitoring_address") or "").strip()
        if explicit:
            return explicit
        ip = request.ip_config.strip()
        if ip and ip.lower() != "dhcp":
            return ip.split("/", 1)[0]
        return None

    @staticmethod
    def _advanced_int(value: object, default: int) -> int:
        try:
            result = int(value) if value is not None else default
        except (TypeError, ValueError):
            return default
        return result if 1 <= result <= 65535 else default
