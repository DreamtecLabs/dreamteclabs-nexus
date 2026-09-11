from __future__ import annotations

from pathlib import Path

import asyncssh
import pytest

from nexus_core.services.ssh_bootstrap import SshBootstrapService


class _RecordingServer(asyncssh.SSHServer):
    def begin_auth(self, username: str) -> bool:
        return False


async def _process_factory(process: asyncssh.SSHServerProcess) -> None:
    stdin_data = await process.stdin.read()
    combined = f"{process.command or ''}{stdin_data}"
    if "FAIL_MARKER" in combined:
        process.stdout.write("boom\n")
        process.exit(1)
        return
    process.stdout.write(combined)
    process.exit(0)


@pytest.fixture
async def ssh_server():
    host_key = asyncssh.generate_private_key("ssh-ed25519")
    server = await asyncssh.listen(
        "127.0.0.1",
        0,
        server_host_keys=[host_key],
        server_factory=_RecordingServer,
        process_factory=_process_factory,
    )
    port = server.sockets[0].getsockname()[1]
    try:
        yield port
    finally:
        server.close()


@pytest.mark.asyncio
async def test_ensure_keypair_generates_once_and_is_stable(tmp_path: Path) -> None:
    key_path = tmp_path / "nexus_ssh_key"
    service = SshBootstrapService(key_path=key_path)
    first = service.ensure_keypair()
    second = service.ensure_keypair()
    assert first == second
    assert first.startswith("ssh-ed25519 ")
    assert key_path.exists()
    assert oct(key_path.stat().st_mode)[-3:] == "600"


@pytest.mark.asyncio
async def test_wait_and_run_executes_script_and_forwards_env(tmp_path: Path, ssh_server: int) -> None:
    service = SshBootstrapService(key_path=tmp_path / "key", attempts=3, interval_seconds=0.1, run_timeout_seconds=5)
    service.ensure_keypair()
    result = await service.wait_and_run(
        "127.0.0.1", port=ssh_server, username="root", script="echo hi", env={"OTEL_VERSION": "1.2.3", "OTLP_HOST": "192.168.0.47"}
    )
    assert result.ok is True
    assert result.detail == "bootstrap script completed"


@pytest.mark.asyncio
async def test_wait_and_run_reports_script_failure(tmp_path: Path, ssh_server: int) -> None:
    service = SshBootstrapService(key_path=tmp_path / "key", attempts=1, interval_seconds=0.1, run_timeout_seconds=5)
    service.ensure_keypair()
    result = await service.wait_and_run("127.0.0.1", port=ssh_server, username="root", script="FAIL_MARKER", env={})
    assert result.ok is False
    assert "exited 1" in result.detail
    assert "boom" in result.detail


@pytest.mark.asyncio
async def test_wait_and_run_retries_until_giving_up(tmp_path: Path) -> None:
    service = SshBootstrapService(key_path=tmp_path / "key", attempts=2, interval_seconds=0.05, run_timeout_seconds=5)
    service.ensure_keypair()
    # Nothing is listening on this port -- every attempt must fail, then give up.
    result = await service.wait_and_run("127.0.0.1", port=1, username="root", script="echo hi", env={})
    assert result.ok is False
    assert "could not reach" in result.detail
