from __future__ import annotations

import asyncio
import re
from datetime import datetime, timezone

from nexus_core.ports.domains import (
    DomainAuditEntry,
    DomainAuditRepository,
    DomainDiagnosticsProvider,
    DomainOperationResult,
    DomainOrchestratorProvider,
    DomainRecord,
    DomainRepository,
    DomainValidation,
)


class DomainError(RuntimeError):
    pass


class DomainOperationsDisabled(DomainError):
    pass


class DomainVerificationFailed(DomainError):
    pass


_DOMAIN_RE = re.compile(r"^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$")
_USER_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")


class DomainService:
    def __init__(
        self,
        repository: DomainRepository,
        diagnostics: DomainDiagnosticsProvider,
        orchestrator: DomainOrchestratorProvider,
        audit: DomainAuditRepository,
        *,
        operations_enabled: bool = False,
        verification_attempts: int = 3,
        verification_interval_seconds: float = 3.0,
        sleep=asyncio.sleep,
    ) -> None:
        self._repository = repository
        self._diagnostics = diagnostics
        self._orchestrator = orchestrator
        self._audit_repository = audit
        self._operations_enabled = operations_enabled
        self._verification_attempts = max(1, verification_attempts)
        self._verification_interval_seconds = max(0.0, verification_interval_seconds)
        self._sleep = sleep

    @property
    def operations_enabled(self) -> bool:
        return self._operations_enabled

    @staticmethod
    def normalize_domain(value: str) -> str:
        domain = value.strip().lower().rstrip(".")
        if not _DOMAIN_RE.fullmatch(domain):
            raise ValueError("invalid domain name")
        return domain

    @staticmethod
    def normalize_hestia_user(value: str) -> str:
        user = value.strip()
        if not _USER_RE.fullmatch(user):
            raise ValueError("invalid Hestia user")
        return user

    def list_domains(self) -> tuple[DomainRecord, ...]:
        return self._repository.list_domains()

    def list_recent_operations(self, limit: int = 25) -> tuple[DomainAuditEntry, ...]:
        return self._audit_repository.list_recent(limit)

    async def validate(self, name: str) -> DomainValidation:
        domain_name = self.normalize_domain(name)
        domain = self._repository.get_domain(domain_name) or DomainRecord(domain_name)
        return await self._diagnostics.validate(domain)

    def _audit(self, *, domain: str, action: str, result: str, detail: str, steps: tuple[str, ...] = ()) -> None:
        self._audit_repository.record(
            DomainAuditEntry(
                timestamp=datetime.now(timezone.utc).isoformat(),
                domain=domain,
                action=action,
                result=result,
                detail=detail[:1000],
                steps=steps,
            )
        )

    async def reconcile(
        self,
        *,
        name: str,
        hestia_user: str = "admin",
        migrate: bool = False,
    ) -> tuple[DomainOperationResult, DomainValidation]:
        if not self._operations_enabled:
            raise DomainOperationsDisabled("Domains & Hosting mutations are disabled by configuration")
        domain_name = self.normalize_domain(name)
        user = self.normalize_hestia_user(hestia_user)
        candidate = DomainRecord(domain_name, True, True, True, True, "pending", user)
        action = "migrate" if migrate else "onboard"
        try:
            result = await self._orchestrator.reconcile(candidate, migrate=migrate)
            self._repository.upsert_domain(candidate)
            validation = await self._diagnostics.validate(candidate)
            for _attempt in range(1, self._verification_attempts):
                if validation.healthy:
                    break
                await self._sleep(self._verification_interval_seconds)
                validation = await self._diagnostics.validate(candidate)
            if not validation.healthy:
                failed = ", ".join(check.key for check in validation.checks if not check.ok) or "unknown"
                self._audit(
                    domain=domain_name,
                    action=action,
                    result="failed-verification",
                    detail=f"post-operation validation failed: {failed}",
                    steps=result.steps,
                )
                raise DomainVerificationFailed(
                    f"{action} completed externally, but Nexus validation still fails: {failed}"
                )
            managed = DomainRecord(domain_name, True, True, True, True, "managed", user)
            self._repository.upsert_domain(managed)
            self._audit(
                domain=domain_name,
                action=action,
                result="success",
                detail="post-operation validation passed",
                steps=result.steps,
            )
            return result, validation
        except Exception as exc:
            if not isinstance(exc, DomainVerificationFailed):
                self._audit(domain=domain_name, action=action, result="failed", detail=str(exc))
            raise
