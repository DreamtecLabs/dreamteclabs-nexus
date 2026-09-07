from __future__ import annotations

import ast
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1] / "src" / "nexus_core"


def imported_modules(path: Path) -> set[str]:
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    modules: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            modules.update(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            modules.add(node.module)
    return modules


def python_files(directory: str) -> list[Path]:
    return sorted(path for path in (ROOT / directory).rglob("*.py") if path.name != "__init__.py")


def assert_no_forbidden_imports(directory: str, forbidden_prefixes: tuple[str, ...]) -> None:
    violations: list[str] = []
    for path in python_files(directory):
        for module in imported_modules(path):
            if any(module == prefix or module.startswith(prefix + ".") for prefix in forbidden_prefixes):
                violations.append(f"{path.relative_to(ROOT)} -> {module}")
    assert not violations, "Architecture boundary violations:\n" + "\n".join(violations)


def test_domain_services_do_not_depend_on_provider_or_ui_implementations() -> None:
    assert_no_forbidden_imports(
        "services",
        (
            "nexus_core.providers",
            "nexus_core.repositories",
            "nexus_core.web",
            "fastapi",
            "jinja2",
        ),
    )


def test_ports_remain_inward_facing_contracts() -> None:
    assert_no_forbidden_imports(
        "ports",
        (
            "nexus_core.providers",
            "nexus_core.repositories",
            "nexus_core.services",
            "nexus_core.web",
            "fastapi",
            "jinja2",
            "httpx",
        ),
    )


def test_provider_adapters_do_not_depend_on_ui() -> None:
    assert_no_forbidden_imports(
        "providers",
        (
            "nexus_core.web",
            "fastapi.templating",
            "jinja2",
        ),
    )
