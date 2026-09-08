from __future__ import annotations

import json
import os
from dataclasses import asdict
from pathlib import Path

from nexus_core.ports.domains import DOMAIN_MODES, DomainRecord


DEFAULT_DOMAINS = (
    DomainRecord("dreamteclabs.com", True, True, True, True, "existing"),
    DomainRecord("kinpilot.app", True, True, True, True, "existing"),
    DomainRecord("savipilot.com", True, True, True, True, "existing"),
    DomainRecord("domuspilot.com", True, True, True, True, "existing"),
    DomainRecord("mundoleo.co", True, True, True, True, "existing"),
    DomainRecord("dreamtec.com.br", True, True, True, False, "existing"),
    DomainRecord("claudiokaist.com", False, False, True, False, "existing"),
)


class JsonDomainRepository:
    def __init__(self, path: Path) -> None:
        self._path = path

    def _seed_if_missing(self) -> None:
        if self._path.exists():
            return
        self._write(DEFAULT_DOMAINS)

    def _read(self) -> tuple[DomainRecord, ...]:
        try:
            self._seed_if_missing()
            raw = json.loads(self._path.read_text(encoding="utf-8"))
            items = raw.get("domains", [])
            domains = []
            for item in items:
                mode = str(item.get("configuration_mode", "discovered"))
                if mode not in DOMAIN_MODES:
                    mode = "discovered"
                domains.append(DomainRecord(
                    name=str(item["name"]).strip().lower(),
                    mail=bool(item.get("mail", False)),
                    webmail=bool(item.get("webmail", False)),
                    ddns=bool(item.get("ddns", False)),
                    tunnel=bool(item.get("tunnel", False)),
                    configuration_mode=mode,
                    hestia_user=str(item.get("hestia_user", "admin")),
                ))
            return tuple(sorted(domains, key=lambda item: item.name))
        except (OSError, ValueError, TypeError, KeyError) as exc:
            raise RuntimeError("Nexus domains inventory is unreadable") from exc

    def _write(self, domains: tuple[DomainRecord, ...] | list[DomainRecord]) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self._path.with_suffix(self._path.suffix + f".{os.getpid()}.tmp")
        payload = {"version": 1, "domains": [asdict(item) for item in sorted(domains, key=lambda item: item.name)]}
        temporary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(self._path)

    def list_domains(self) -> tuple[DomainRecord, ...]:
        return self._read()

    def get_domain(self, name: str) -> DomainRecord | None:
        normalized = name.strip().lower().rstrip(".")
        return next((item for item in self._read() if item.name == normalized), None)

    def upsert_domain(self, domain: DomainRecord) -> DomainRecord:
        current = [item for item in self._read() if item.name != domain.name]
        current.append(domain)
        self._write(current)
        return domain
