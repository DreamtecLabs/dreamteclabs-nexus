from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from datetime import datetime, timezone

from nexus_core.ports.infrastructure import (
    POWER_ACTIONS,
    PVE_GUEST_TYPES,
    InfrastructureProvider,
    InfrastructureResource,
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


class InfrastructureService:
    _allowed_from = {
        "start": frozenset({"stopped"}),
        "shutdown": frozenset({"running"}),
        "stop": frozenset({"running"}),
    }
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
    ) -> None:
        self._provider = provider
        self._power_operations_enabled = power_operations_enabled
        self._verification_attempts = max(1, verification_attempts)
        self._verification_interval_seconds = max(0.0, verification_interval_seconds)
        self._audit_repository = audit_repository
        self._sleep = sleep

    @property
    def power_operations_enabled(self) -> bool:
        return self._power_operations_enabled

    async def list_resources(self) -> InfrastructureSnapshot:
        return await self._provider.list_resources()

    def list_recent_power_operations(self, limit: int = 25) -> tuple[PowerAuditEntry, ...]:
        if self._audit_repository is None:
            return ()
        return self._audit_repository.list_recent(limit)

    async def get_resource(self, resource_id: str) -> InfrastructureResource:
        resource_id = resource_id.strip()
        snapshot = await self.list_resources()
        for resource in snapshot.resources:
            if resource.id == resource_id:
                return resource
        raise InfrastructureResourceNotFound(f"Infrastructure resource '{resource_id}' was not found")

    def available_power_actions(self, resource: InfrastructureResource) -> dict[str, bool]:
        status = resource.status.strip().lower()
        supported_guest = resource.type in PVE_GUEST_TYPES and resource.vmid is not None and resource.template is not True
        return {
            action: bool(supported_guest and status in permitted)
            for action, permitted in self._allowed_from.items()
        }

    def _audit(
        self,
        *,
        resource: InfrastructureResource,
        action: str,
        result: str,
        detail: str,
        task_reference: str | None,
    ) -> None:
        if self._audit_repository is None:
            return
        self._audit_repository.record(
            PowerAuditEntry(
                timestamp=datetime.now(timezone.utc).isoformat(),
                resource_id=resource.id,
                resource_name=resource.name,
                action=action,
                result=result,
                detail=detail[:1000],
                task_reference=task_reference,
            )
        )

    async def execute_power_action(
        self,
        *,
        resource_id: str,
        action: str,
        confirmation: str | None = None,
    ) -> PowerOperationResult:
        if not self._power_operations_enabled:
            raise PowerOperationsDisabled("Infrastructure power operations are disabled by configuration")

        action = action.strip().lower()
        if action not in POWER_ACTIONS:
            raise PowerActionNotAllowed(f"Unsupported power action '{action}'")

        resource = await self.get_resource(resource_id)
        available = self.available_power_actions(resource)
        if not available.get(action, False):
            raise PowerActionNotAllowed(
                f"{action} is not available for {resource.name} while status is {resource.status}"
            )

        if action == "stop" and (confirmation or "").strip() != resource.name:
            raise PowerActionNotAllowed(
                f"Hard stop requires confirmation with the exact resource name '{resource.name}'"
            )

        task_reference: str | None = None
        try:
            task_reference = await self._provider.execute_power_action(resource, action)
            expected = self._expected_status[action]
            observed = resource.status

            for attempt in range(self._verification_attempts):
                snapshot = await self.list_resources()
                current = next((item for item in snapshot.resources if item.id == resource.id), None)
                if current is None:
                    raise InfrastructureResourceNotFound(
                        f"Infrastructure resource '{resource.id}' disappeared during power verification"
                    )
                observed = current.status
                if observed == expected:
                    result = PowerOperationResult(
                        resource_id=resource.id,
                        resource_name=resource.name,
                        action=action,
                        task_reference=task_reference,
                        expected_status=expected,
                        observed_status=observed,
                        verified=True,
                    )
                    self._audit(
                        resource=resource,
                        action=action,
                        result="success",
                        detail=f"Verified final status '{observed}'",
                        task_reference=task_reference,
                    )
                    return result
                if attempt + 1 < self._verification_attempts:
                    await self._sleep(self._verification_interval_seconds)

            raise PowerVerificationTimeout(
                f"PDM accepted {action} for {resource.name}, but Nexus did not observe status '{expected}' "
                f"after {self._verification_attempts} checks (last status: '{observed}')"
            )
        except Exception as exc:
            self._audit(
                resource=resource,
                action=action,
                result="failed",
                detail=str(exc),
                task_reference=task_reference,
            )
            raise
