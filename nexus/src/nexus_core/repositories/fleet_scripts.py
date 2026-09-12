from __future__ import annotations

from pathlib import Path

from nexus_core.ports.fleet import FleetScript

_MARKER_PREFIX = "# nexus-fleet:"


class FilesystemFleetScriptRepository:
    """Discovers fleet-pushable scripts by scanning a directory for an opt-in header.

    Only ``.sh`` files carrying a leading ``# nexus-fleet:name=...`` comment block
    are listed -- everything else in the directory (e.g. nexus-domains-helper) is
    ignored. New scripts become available the moment the file lands on disk, with
    no Nexus Core deploy required, matching how nexus-otel-lxc-agent.sh is already
    read fresh from the git checkout.
    """

    def __init__(self, directory: Path) -> None:
        self._directory = directory

    def list_scripts(self) -> list[FleetScript]:
        if not self._directory.is_dir():
            return []
        scripts = (self._parse(path) for path in sorted(self._directory.glob("*.sh")))
        return [script for script in scripts if script is not None]

    def read_script(self, path: str) -> str:
        return self._resolve(path).read_text()

    def _resolve(self, path: str) -> Path:
        directory = self._directory.resolve()
        resolved = (directory / path).resolve()
        if resolved.parent != directory or not resolved.is_file():
            raise KeyError(path)
        return resolved

    @staticmethod
    def _parse(path: Path) -> FleetScript | None:
        fields: dict[str, str] = {}
        try:
            with path.open("r", encoding="utf-8") as handle:
                for line in handle:
                    line = line.strip()
                    if not line.startswith(_MARKER_PREFIX):
                        if fields:
                            break
                        continue
                    key, _, value = line[len(_MARKER_PREFIX):].partition("=")
                    fields[key.strip()] = value.strip()
        except OSError:
            return None
        name = fields.get("name")
        if not name:
            return None
        params = tuple(part.strip() for part in fields.get("params", "").split(",") if part.strip())
        return FleetScript(name=name, path=path.name, description=fields.get("description", ""), params=params)
