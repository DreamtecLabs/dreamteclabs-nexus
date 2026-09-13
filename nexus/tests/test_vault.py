from __future__ import annotations

from pathlib import Path

import pytest
from cryptography.fernet import Fernet
from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app
from nexus_core.repositories.vault_audit_jsonl import JsonlVaultAuditRepository
from nexus_core.repositories.vault_json import JsonVaultRepository
from nexus_core.services.vault import VaultService, ensure_vault_key


def test_ensure_vault_key_generates_once_and_is_stable(tmp_path: Path) -> None:
    key_path = tmp_path / "vault.key"
    first = ensure_vault_key(key_path)
    second = ensure_vault_key(key_path)
    assert first == second
    assert key_path.exists()
    assert oct(key_path.stat().st_mode)[-3:] == "600"
    Fernet(first)  # raises if it isn't a valid Fernet key


def test_repository_never_exposes_plaintext(tmp_path: Path) -> None:
    cipher = Fernet(ensure_vault_key(tmp_path / "vault.key"))
    repository = JsonVaultRepository(tmp_path / "vault.json")
    service = VaultService(repository, cipher)

    service.create_secret(name="signoz-api-key", value="top-secret-value", type="api-token", notes="rotates yearly")

    raw = (tmp_path / "vault.json").read_text()
    assert "top-secret-value" not in raw
    record = repository.get_secret("signoz-api-key")
    assert record is not None
    assert record.ciphertext != "top-secret-value"
    assert cipher.decrypt(record.ciphertext.encode("ascii")).decode("utf-8") == "top-secret-value"


def test_repository_file_permissions_are_restricted(tmp_path: Path) -> None:
    cipher = Fernet(ensure_vault_key(tmp_path / "vault.key"))
    repository = JsonVaultRepository(tmp_path / "vault.json")
    VaultService(repository, cipher).create_secret(name="x", value="y")
    assert oct((tmp_path / "vault.json").stat().st_mode)[-3:] == "600"


def _service(tmp_path: Path) -> VaultService:
    cipher = Fernet(ensure_vault_key(tmp_path / "vault.key"))
    return VaultService(JsonVaultRepository(tmp_path / "vault.json"), cipher, JsonlVaultAuditRepository(tmp_path / "audit.jsonl"))


def test_create_secret_rejects_duplicate_name(tmp_path: Path) -> None:
    service = _service(tmp_path)
    service.create_secret(name="dup", value="one")
    with pytest.raises(ValueError, match="already exists"):
        service.create_secret(name="dup", value="two")


def test_create_secret_requires_name_and_value(tmp_path: Path) -> None:
    service = _service(tmp_path)
    with pytest.raises(ValueError, match="name is required"):
        service.create_secret(name="  ", value="x")
    with pytest.raises(ValueError, match="value is required"):
        service.create_secret(name="x", value="")


def test_reveal_secret_returns_original_value_and_logs_audit(tmp_path: Path) -> None:
    service = _service(tmp_path)
    service.create_secret(name="cf-token", value="cf-abc123", type="api-token")

    value = service.reveal_secret("cf-token")

    assert value == "cf-abc123"
    actions = [entry.action for entry in service.list_audit()]
    assert actions == ["reveal", "create"]


def test_reveal_missing_secret_raises_key_error(tmp_path: Path) -> None:
    service = _service(tmp_path)
    with pytest.raises(KeyError):
        service.reveal_secret("missing")


def test_update_secret_rotates_value_and_keeps_created_at(tmp_path: Path) -> None:
    service = _service(tmp_path)
    created = service.create_secret(name="rotating", value="v1")

    updated = service.update_secret("rotating", value="v2")

    assert updated.created_at == created.created_at
    assert updated.updated_at != created.updated_at
    assert service.reveal_secret("rotating") == "v2"


def test_update_secret_notes_only_keeps_value(tmp_path: Path) -> None:
    service = _service(tmp_path)
    service.create_secret(name="s", value="unchanged")

    service.update_secret("s", notes="new note")

    assert service.reveal_secret("s") == "unchanged"
    assert service.list_secrets()[0].notes == "new note"


def test_update_missing_secret_raises_key_error(tmp_path: Path) -> None:
    service = _service(tmp_path)
    with pytest.raises(KeyError):
        service.update_secret("missing", value="x")


def test_delete_secret_removes_it(tmp_path: Path) -> None:
    service = _service(tmp_path)
    service.create_secret(name="temp", value="x")
    service.delete_secret("temp")
    assert service.list_secrets() == []
    with pytest.raises(KeyError):
        service.reveal_secret("temp")


def test_list_secrets_is_sorted_and_case_insensitive(tmp_path: Path) -> None:
    service = _service(tmp_path)
    service.create_secret(name="zebra", value="1")
    service.create_secret(name="Alpha", value="2")
    assert [secret.name for secret in service.list_secrets()] == ["Alpha", "zebra"]


def _app(tmp_path: Path):
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None)
    return create_app(settings)


def test_vault_create_and_list_routes(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        create_response = client.post("/api/v1/vault", json={"name": "api-key", "value": "secretvalue", "type": "api-token", "notes": "n"})
        assert create_response.status_code == 200
        list_response = client.get("/api/v1/vault")
        assert list_response.status_code == 200
        names = [s["name"] for s in list_response.json()["secrets"]]
        assert names == ["api-key"]


def test_vault_create_route_rejects_duplicate(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        client.post("/api/v1/vault", json={"name": "dup", "value": "x"})
        response = client.post("/api/v1/vault", json={"name": "dup", "value": "y"})
    assert response.status_code == 422


def test_vault_reveal_route_returns_plaintext(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        client.post("/api/v1/vault", json={"name": "k", "value": "plaintext-value"})
        response = client.post("/api/v1/vault/k/reveal")
    assert response.status_code == 200
    assert response.json()["value"] == "plaintext-value"


def test_vault_reveal_route_missing_secret_404(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        response = client.post("/api/v1/vault/missing/reveal")
    assert response.status_code == 404


def test_vault_update_route(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        client.post("/api/v1/vault", json={"name": "k", "value": "v1"})
        update_response = client.put("/api/v1/vault/k", json={"notes": "updated"})
        assert update_response.status_code == 200
        assert update_response.json()["notes"] == "updated"


def test_vault_delete_route(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        client.post("/api/v1/vault", json={"name": "k", "value": "v1"})
        delete_response = client.delete("/api/v1/vault/k")
        assert delete_response.status_code == 200
        list_response = client.get("/api/v1/vault")
        assert list_response.json()["secrets"] == []


def test_vault_page_renders(tmp_path: Path) -> None:
    app = _app(tmp_path)
    with TestClient(app) as client:
        client.post("/api/v1/vault", json={"name": "page-secret", "value": "unique-plaintext-xyz", "notes": "shown here"})
        response = client.get("/vault")
    assert response.status_code == 200
    assert "Vault" in response.text
    assert "page-secret" in response.text
    assert "unique-plaintext-xyz" not in response.text  # plaintext value never rendered server-side, only via a separate reveal fetch
