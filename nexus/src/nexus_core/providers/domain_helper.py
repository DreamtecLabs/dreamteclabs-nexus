from __future__ import annotations

import asyncio
import json
from pathlib import Path

from nexus_core.ports.domains import DomainOperationResult, DomainRecord


class DomainHelperProvider:
    def __init__(self, helper_path: Path, timeout_seconds: float = 300.0) -> None:
        self._helper_path = helper_path
        self._timeout_seconds = timeout_seconds

    async def reconcile(self, domain: DomainRecord, *, migrate: bool) -> DomainOperationResult:
        action = "migrate" if migrate else "onboard"
        if not self._helper_path.is_file():
            raise RuntimeError(f"Domains helper is not available at {self._helper_path}")
        try:
            process = await asyncio.create_subprocess_exec(
                str(self._helper_path), action, domain.name, domain.hestia_user,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
            )
            stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=self._timeout_seconds)
        except TimeoutError as exc:
            raise RuntimeError(f"Domains helper timed out during {action}") from exc
        except OSError as exc:
            raise RuntimeError(f"Domains helper failed to start: {type(exc).__name__}") from exc
        if process.returncode != 0:
            detail = stderr.decode("utf-8", errors="replace").strip() or stdout.decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"Domains helper {action} failed (rc={process.returncode}): {detail[:500] or 'no diagnostic output'}")
        try:
            payload = json.loads(stdout.decode("utf-8"))
        except (UnicodeDecodeError, ValueError) as exc:
            raise RuntimeError("Domains helper returned invalid JSON") from exc
        if payload.get("ok") is not True:
            raise RuntimeError(f"Domains helper {action} did not report success")
        steps = tuple(str(step) for step in payload.get("steps", []) if isinstance(step, str))
        return DomainOperationResult(domain=domain.name, action=action, ok=True, steps=steps)
