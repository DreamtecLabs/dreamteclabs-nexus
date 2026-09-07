from pathlib import Path

from nexus_core.ports.infrastructure import PowerAuditEntry
from nexus_core.repositories.power_audit_jsonl import JsonlPowerAuditRepository


def _entry(timestamp: str, name: str, result: str) -> PowerAuditEntry:
    return PowerAuditEntry(
        timestamp=timestamp,
        resource_id=f"remote/home-pve/guest/{name}",
        resource_name=name,
        action="start",
        result=result,
        detail=f"{result} detail",
        task_reference="UPID:test" if result == "success" else None,
    )


def test_power_audit_is_append_only_and_recent_first(tmp_path: Path) -> None:
    repository = JsonlPowerAuditRepository(tmp_path / "power-operations.jsonl")
    first = _entry("2026-09-07T20:00:00+00:00", "101", "success")
    second = _entry("2026-09-07T20:01:00+00:00", "102", "failed")

    repository.record(first)
    repository.record(second)

    assert repository.list_recent() == (second, first)
    assert repository.list_recent(1) == (second,)
    assert (tmp_path / "power-operations.jsonl").read_text(encoding="utf-8").count("\n") == 2


def test_power_audit_skips_malformed_historical_lines(tmp_path: Path) -> None:
    path = tmp_path / "power-operations.jsonl"
    path.write_text("not-json\n", encoding="utf-8")
    repository = JsonlPowerAuditRepository(path)
    valid = _entry("2026-09-07T20:02:00+00:00", "103", "success")
    repository.record(valid)

    assert repository.list_recent() == (valid,)
