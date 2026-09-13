from __future__ import annotations

from pathlib import Path

import pytest
from fastapi import Request
from fastapi.testclient import TestClient

from nexus_core.auth import _is_protected, _safe_next, ensure_session_secret
from nexus_core.config import Settings
from nexus_core.main import create_app


def test_ensure_session_secret_generates_once_and_is_stable(tmp_path: Path) -> None:
    path = tmp_path / "session-secret.key"
    first = ensure_session_secret(path)
    second = ensure_session_secret(path)
    assert first == second
    assert len(first) >= 32
    assert oct(path.stat().st_mode)[-3:] == "600"


@pytest.mark.parametrize(
    "path,expected",
    [
        ("/login", False),
        ("/auth/callback", False),
        ("/logout", False),
        ("/health", False),
        ("/static/nexus.css", False),
        ("/", True),
        ("/vault", True),
        ("/api/v1/ipam", True),
    ],
)
def test_is_protected(path: str, expected: bool) -> None:
    assert _is_protected(path) is expected


@pytest.mark.parametrize(
    "value,expected",
    [
        (None, "/"),
        ("", "/"),
        ("/vault", "/vault"),
        ("/vault?x=1", "/vault?x=1"),
        ("//evil.example.com/phish", "/"),
        ("https://evil.example.com", "/"),
        ("not-a-path", "/"),
    ],
)
def test_safe_next(value: str | None, expected: str) -> None:
    assert _safe_next(value) == expected


def _oidc_settings(tmp_path: Path, **overrides) -> Settings:
    return Settings(
        NEXUS_DATA_DIR=tmp_path,
        PDM_BASE_URL="https://pdm.invalid",
        PDM_VERIFY_TLS=False,
        NEXUS_SIGNOZ_API_KEY=None,
        NEXUS_AUTH_ENABLED=True,
        NEXUS_OIDC_ISSUER="https://auth.example.com/application/o/nexus/",
        NEXUS_OIDC_CLIENT_ID="nexus",
        NEXUS_OIDC_CLIENT_SECRET="test-secret",
        **overrides,
    )


def test_auth_enabled_requires_oidc_settings(tmp_path: Path) -> None:
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None, NEXUS_AUTH_ENABLED=True)
    with pytest.raises(RuntimeError, match="requires NEXUS_OIDC"):
        create_app(settings)


def test_auth_disabled_by_default_leaves_pages_open(tmp_path: Path) -> None:
    settings = Settings(NEXUS_DATA_DIR=tmp_path, PDM_BASE_URL="https://pdm.invalid", PDM_VERIFY_TLS=False, NEXUS_SIGNOZ_API_KEY=None)
    app = create_app(settings)
    with TestClient(app) as client:
        response = client.get("/vault")
    assert response.status_code == 200


def test_unauthenticated_page_redirects_to_login(tmp_path: Path) -> None:
    app = create_app(_oidc_settings(tmp_path))
    with TestClient(app, follow_redirects=False) as client:
        response = client.get("/vault")
    assert response.status_code in (302, 307)
    assert response.headers["location"] == "/login?next=/vault"


def test_unauthenticated_api_returns_401_json(tmp_path: Path) -> None:
    app = create_app(_oidc_settings(tmp_path))
    with TestClient(app) as client:
        response = client.get("/api/v1/vault")
    assert response.status_code == 401
    assert response.json()["detail"] == "authentication required"


def test_health_stays_open_when_auth_enabled(tmp_path: Path) -> None:
    app = create_app(_oidc_settings(tmp_path))
    with TestClient(app) as client:
        response = client.get("/health")
    assert response.status_code == 200


async def _fake_authorize_access_token(request: Request) -> dict[str, object]:
    return {"access_token": "fake", "userinfo": {"sub": "u1", "email": "claudio@example.com", "name": "Claudio Kaist"}}


def test_callback_sets_session_and_redirects_to_target(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    app = create_app(_oidc_settings(tmp_path))
    monkeypatch.setattr(app.state.oauth_client, "authorize_access_token", _fake_authorize_access_token)

    with TestClient(app, follow_redirects=False) as client:
        # Establishes session["post_login_redirect"] the same way /login would,
        # without making a real network call to the (fake) OIDC provider.
        client.get("/vault")  # redirected, but also seeds the middleware's session cookie
        callback_response = client.get("/auth/callback")
        assert callback_response.status_code in (302, 307)
        vault_response = client.get("/vault")
    assert vault_response.status_code == 200


def test_logout_clears_session(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    app = create_app(_oidc_settings(tmp_path))
    monkeypatch.setattr(app.state.oauth_client, "authorize_access_token", _fake_authorize_access_token)

    with TestClient(app, follow_redirects=False) as client:
        client.get("/auth/callback")
        assert client.get("/vault").status_code == 200
        client.get("/logout")
        response = client.get("/vault")
    assert response.status_code in (302, 307)
