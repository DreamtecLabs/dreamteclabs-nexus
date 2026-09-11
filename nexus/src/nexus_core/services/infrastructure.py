from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from datetime import datetime, timezone

from nexus_core.ports.infrastructure import (
    POWER_ACTIONS,
    PVE_GUEST_TYPES,
    DecommissionResult,
    InfrastructureNodeSummary,
    InfrastructureProvider,
    InfrastructureRemoteSummary,
    InfrastructureResource,
    InfrastructureResourceContext,
    InfrastructureSnapshot,
    PowerAuditEntry,
    PowerAuditRepository,
    PowerOperationResult,
)


class InfrastructureError(RuntimeError):
    pass


class InfrastructureResourceNotFound(InfrastructureError):
    pass


class PowerOperationsDisabled(InfrastructureError):
    pass


class PowerActionNotAllowed(InfrastructureError):
    pass


class PowerVerificationTimeout(InfrastructureError):
    pass


class DecommissionDisabled(InfrastructureError):
    pass


class DecommissionNotAllowed(InfrastructureError):
    pass


class DecommissionVerificationTimeout(InfrastructureError):
    pass


class InfrastructureService:
    _allowed_from = {"start": frozenset({"stopped"}), "shutdown": frozenset({"running"}), "stop": frozenset({"running"})}
    _expected_status = {"start": "running", "shutdown": "stopped", "stop": "stopped"}

    def __init__(
        self,
        provider: InfrastructureProvider,
        *,
        power_operations_enabled: bool = False,
        verification_attempts: int = 20,
        verification_interval_seconds: float = 1.0,
        audit_repository: PowerAuditRepository | None = None,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
        decommission_enabled: bool = False,
        monitoring_service=None,
    ) -> None:
        self._provider = provider
        self._power_operations_enabled = power_operations_enabled
        self._verification_attempts = max(1, verification_attempts)
        self._verification_interval_seconds = max(0.0, verification_interval_seconds)
        self._audit_repository = audit_repository
        self._sleep = sleep
        self._decommission_enabled = decommission_enabled
        self._monitoring = monitoring_service

    @property
    def power_operations_enabled(self) -> bool:
        return self._power_operations_enabled

    @property
    def decommission_enabled(self) -> bool:
        return self._decommission_enabled

    async def list_resources(self) -> InfrastructureSnapshot:
        return await self._provider.list_resources()

    def list_recent_power_operations(self, limit: int = 25) -> tuple[PowerAuditEntry, ...]:
        return () if self._audit_repository is None else self._audit_repository.list_recent(limit)

    async def get_resource(self, resource_id: str) -> InfrastructureResource:
        resource_id = resource_id.strip()
        snapshot = await self.list_resources()
        return self._find_resource(snapshot, resource_id)

    async def get_resource_context(self, resource_id: str) -> InfrastructureResourceContext:
        snapshot = await self.list_resources()
        resource = self._find_resource(snapshot, resource_id.strip())
        same_remote = tuple(item for item in snapshot.resources if item.remote == resource.remote and item.id != resource.id)
        node_name = resource.name if resource.type == "pve-node" else resource.node
        node = next((item for item in snapshot.resources if item.remote == resource.remote and item.type == "pve-node" and item.name == node_name), None)
        if resource.type.startswith("pbs-"):
            node = next((item for item in snapshot.resources if item.remote == resource.remote and item.type == "pbs-node"), None)
        guests = tuple(item for item in same_remote if item.type in PVE_GUEST_TYPES and (node_name is None or item.node == node_name))
        storages = tuple(item for item in same_remote if item.type in {"pve-storage", "pbs-datastore"} and (node_name is None or item.node in {None, node_name}))
        networks = tuple(item for item in same_remote if item.type == "pve-network" and (node_name is None or item.node in {None, node_name}))
        related_ids = {item.id for item in guests + storages + networks}
        if node is not None:
            related_ids.add(node.id)
        related = tuple(item for item in same_remote if item.id in related_ids)
        return InfrastructureResourceContext(resource=resource, node=node, guests=guests, storages=storages, networks=networks, related=related)

    async def estate_summary(self) -> tuple[InfrastructureRemoteSummary, ...]:
        snapshot = await self.list_resources()
        summaries: list[InfrastructureRemoteSummary] = []
        for remote in sorted({item.remote for item in snapshot.resources}, key=str.casefold):
            items = tuple(item for item in snapshot.resources if item.remote == remote)
            guests = tuple(item for item in items if item.type in PVE_GUEST_TYPES)
            node_summaries: list[InfrastructureNodeSummary] = []
            for node in sorted((item for item in items if item.type == "pve-node"), key=lambda item: item.name.casefold()):
                node_guests = tuple(item for item in guests if item.node == node.name)
                node_summaries.append(InfrastructureNodeSummary(name=node.name, status=node.status, guests=len(node_guests), running=sum(item.status == "running" for item in node_guests), stopped=sum(item.status == "stopped" for item in node_guests), cpu_usage=node.cpu_usage, cpu_total=node.cpu_total, memory_used_bytes=node.memory_used_bytes, memory_total_bytes=node.memory_total_bytes, uptime_seconds=node.uptime_seconds))
            summaries.append(InfrastructureRemoteSummary(remote=remote, resources=len(items), guests=len(guests), running=sum(item.status == "running" for item in guests), stopped=sum(item.status == "stopped" for item in guests), pve_nodes=sum(item.type == "pve-node" for item in items), pbs_resources=sum(item.type.startswith("pbs-") for item in items), storages=sum(item.type in {"pve-storage", "pbs-datastore"} for item in items), networks=sum(item.type == "pve-network" for item in items), nodes=tuple(node_summaries)))
        return tuple(summaries)

    @staticmethod
    def _find_resource(snapshot: InfrastructureSnapshot, resource_id: str) -> InfrastructureResource:
        for resource in snapshot.resources:
            if resource.id == resource_id:
                return resource
        raise InfrastructureResourceNotFound(f"Infrastructure resource '{resource_id}' was not found")

    def available_power_actions(self, resource: InfrastructureResource) -> dict[str, bool]:
        status = resource.status.strip().lower()
        supported_guest = resource.type in PVE_GUEST_TYPES and resource.vmid is not None and resource.template is not True
        return {action: bool(supported_guest and status in permitted) for action, permitted in self._allowed_from.items()}

    def _audit(self, *, resource: InfrastructureResource, action: str, result: str, detail: str, task_reference: str | None) -> None:
        if self._audit_repository is None:
            return
        self._audit_repository.record(PowerAuditEntry(timestamp=datetime.now(timezone.utc).isoformat(), resource_id=resource.id, resource_name=resource.name, action=action, result=result, detail=detail[:1000], task_reference=task_reference))

    async def execute_power_action(self, *, resource_id: str, action: str, confirmation: str | None = None) -> PowerOperationResult:
        if not self._power_operations_enabled:
            raise PowerOperationsDisabled("Infrastructure power operations are disabled by configuration")
        action = action.strip().lower()
        if action not in POWER_ACTIONS:
            raise PowerActionNotAllowed(f"Unsupported power action '{action}'")
        resource = await self.get_resource(resource_id)
        available = self.available_power_actions(resource)
        if not available.get(action, False):
            raise PowerActionNotAllowed(f"{action} is not available for {resource.name} while status is {resource.status}")
        if action == "stop" and (confirmation or "").strip() != resource.name:
            raise PowerActionNotAllowed(f"Hard stop requires confirmation with the exact resource name '{resource.name}'")
        task_reference: str | None = None
        try:
            task_reference = await self._provider.execute_power_action(resource, action)
            expected = self._expected_status[action]
            observed = resource.status
            for attempt in range(self._verification_attempts):
                snapshot = await self.list_resources()
                current = next((item for item in snapshot.resources if item.id == resource.id), None)
                if current is None:
                    raise InfrastructureResourceNotFound(f"Infrastructure resource '{resource.id}' disappeared during power verification")
                observed = current.status
                if observed == expected:
                    result = PowerOperationResult(resource_id=resource.id, resource_name=resource.name, action=action, task_reference=task_reference, expected_status=expected, observed_status=observed, verified=True)
                    self._audit(resource=resource, action=action, result="success", detail=f"Verified final status '{observed}'", task_reference=task_reference)
                    return result
                if attempt + 1 < self._verification_attempts:
                    await self._sleep(self._verification_interval_seconds)
            raise PowerVerificationTimeout(f"PDM accepted {action} for {resource.name}, but Nexus did not observe status '{expected}' after {self._verification_attempts} checks (last status: '{observed}')")
        except Exception as exc:
            self._audit(resource=resource, action=action, result="failed", detail=str(exc), task_reference=task_reference)
            raise

    async def decommission(self, *, resource_id: str, confirmation: str, purge: bool = True) -> DecommissionResult:
        if not self._decommission_enabled:
            raise DecommissionDisabled("Guest decommissioning is disabled by configuration")
        resource = await self.get_resource(resource_id)
        supported = resource.type in PVE_GUEST_TYPES and resource.vmid is not None
        if not supported or resource.template is True:
            raise DecommissionNotAllowed(f"{resource.name} cannot be decommissioned (not a destroyable guest)")
        if confirmation.strip() != resource.name:
            raise DecommissionNotAllowed(f"Decommission requires confirmation with the exact resource name '{resource.name}'")
        task_reference: str | None = None
        try:
            if resource.status == "running":
                # Stopping first is an internal step of the (already gated) decommission
                # flow, not a user-facing power action -- go straight to the provider
                # instead of execute_power_action() so this never depends on the separate
                # NEXUS_POWER_OPERATIONS_ENABLED flag.
                await self._provider.execute_power_action(resource, "stop")
                for attempt in range(self._verification_attempts):
                    snapshot = await self.list_resources()
                    current = next((item for item in snapshot.resources if item.id == resource.id), None)
                    if current is None or current.status == "stopped":
                        break
                    if attempt + 1 < self._verification_attempts:
                        await self._sleep(self._verification_interval_seconds)
            task_reference = await self._provider.destroy_guest(resource, purge=purge)
            for attempt in range(self._verification_attempts):
                snapshot = await self.list_resources()
                if not any(item.id == resource.id for item in snapshot.resources):
                    break
                if attempt + 1 < self._verification_attempts:
                    await self._sleep(self._verification_interval_seconds)
            else:
                raise DecommissionVerificationTimeout(f"PDM accepted destroy for {resource.name}, but Nexus still observed it after {self._verification_attempts} checks")
            monitoring_removed = False
            if self._monitoring is not None:
                monitoring_removed = await self._monitoring.delete_target_by_name(resource.name)
            result = DecommissionResult(resource_id=resource.id, resource_name=resource.name, task_reference=task_reference, verified=True, monitoring_target_removed=monitoring_removed)
            self._audit(resource=resource, action="decommission", result="success", detail=f"Destroyed and verified absent (monitoring cleanup: {'removed' if monitoring_removed else 'nothing registered'})", task_reference=task_reference)
            return result
        except Exception as exc:
            self._audit(resource=resource, action="decommission", result="failed", detail=str(exc), task_reference=task_reference)
            raise
