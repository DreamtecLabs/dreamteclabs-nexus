from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from pathlib import Path

import asyncssh


@dataclass(frozen=True, slots=True)
class BootstrapResult:
    ok: bool
    detail: str


class SshBootstrapService:
    """Runs Nexus-authored scripts over SSH on freshly provisioned guests.

    Nexus owns a dedicated ed25519 keypair (generated once, on first use)
    so it can reach a guest it just created without depending on any
    password or key the operator may separately choose to inject.
    """

    def __init__(
        self,
        *,
        key_path: Path,
        attempts: int = 30,
        interval_seconds: float = 5.0,
        run_timeout_seconds: float = 180.0,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
    ) -> None:
        self._key_path = key_path
        self._attempts = max(1, attempts)
        self._interval_seconds = max(0.1, interval_seconds)
        self._run_timeout_seconds = max(1.0, run_timeout_seconds)
        self._sleep = sleep

    def ensure_keypair(self) -> str:
        """Generate the Nexus SSH keypair on first use and return the public key."""
        public_path = self._key_path.with_suffix(".pub")
        if not self._key_path.exists():
            self._key_path.parent.mkdir(parents=True, exist_ok=True)
            key = asyncssh.generate_private_key("ssh-ed25519", comment="nexus-provisioning")
            key.write_private_key(str(self._key_path))
            self._key_path.chmod(0o600)
            key.write_public_key(str(public_path))
        return public_path.read_text().strip()

    async def wait_and_run(self, address: str, *, username: str, script: str, env: dict[str, str], port: int = 22) -> BootstrapResult:
        last_error: Exception | None = None
        for attempt in range(self._attempts):
            try:
                async with asyncssh.connect(
                    address,
                    port=port,
                    username=username,
                    client_keys=[str(self._key_path)],
                    known_hosts=None,
                    connect_timeout=10,
                ) as conn:
                    return await self._run_script(conn, script, env)
            except (OSError, asyncssh.Error) as exc:
                last_error = exc
                if attempt + 1 < self._attempts:
                    await self._sleep(self._interval_seconds)
        return BootstrapResult(False, f"could not reach {address} over SSH: {last_error}")

    async def _run_script(self, conn: asyncssh.SSHClientConnection, script: str, env: dict[str, str]) -> BootstrapResult:
        prefix = "".join(f"export {key}={self._shell_quote(value)}\n" for key, value in env.items())
        try:
            result = await conn.run(f"{prefix}bash -s", input=script, check=False, timeout=self._run_timeout_seconds)
        except asyncssh.TimeoutError:
            return BootstrapResult(False, "bootstrap script timed out")
        output = "\n".join(part for part in (result.stdout, result.stderr) if part).strip()
        if result.exit_status != 0:
            tail = output[-1000:] if output else "no output"
            return BootstrapResult(False, f"bootstrap script exited {result.exit_status}: {tail}")
        return BootstrapResult(True, "bootstrap script completed")

    @staticmethod
    def _shell_quote(value: str) -> str:
        return "'" + value.replace("'", "'\\''") + "'"
