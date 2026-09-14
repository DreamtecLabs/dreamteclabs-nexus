from __future__ import annotations

import secrets
from pathlib import Path

from authlib.integrations.starlette_client import OAuth
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, RedirectResponse
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.middleware.sessions import SessionMiddleware

from nexus_core.config import Settings

_UNPROTECTED_PATHS = frozenset({"/login", "/auth/callback", "/logout", "/health"})
_UNPROTECTED_PREFIXES = ("/static/",)


def ensure_session_secret(path: Path) -> str:
    """Generate the cookie-signing secret once -- same lazy on-disk-once
    pattern as SshBootstrapService.ensure_keypair() and VaultService's key."""
    if path.exists():
        return path.read_text().strip()
    path.parent.mkdir(parents=True, exist_ok=True)
    secret = secrets.token_urlsafe(32)
    path.write_text(secret)
    path.chmod(0o600)
    return secret


def _is_protected(path: str) -> bool:
    if path in _UNPROTECTED_PATHS:
        return False
    return not path.startswith(_UNPROTECTED_PREFIXES)


def _safe_next(value: str | None) -> str:
    """Only ever redirect to a same-site relative path. Both '//host/x' and
    '/\\host/x' are browser-parsed as protocol-relative (backslash is
    normalized to slash by the WHATWG URL parser for http(s) URLs) and would
    send the login result off-site, so both leading forms are rejected."""
    if not value or not value.startswith("/") or (len(value) > 1 and value[1] in ("/", "\\")):
        return "/"
    return value


class RequireLoginMiddleware(BaseHTTPMiddleware):
    """Fail-closed: every path is protected unless explicitly allow-listed.
    Must be installed so SessionMiddleware runs first (Starlette executes the
    *last*-added middleware first), or request.session isn't populated yet."""

    async def dispatch(self, request: Request, call_next):
        if _is_protected(request.url.path) and not request.session.get("user"):
            if request.url.path.startswith("/api/"):
                return JSONResponse({"detail": "authentication required"}, status_code=401)
            next_path = request.url.path + (f"?{request.url.query}" if request.url.query else "")
            return RedirectResponse(url=f"/login?next={next_path}")
        return await call_next(request)


def install_auth(app: FastAPI, settings: Settings) -> None:
    """Optional OIDC login (Authentik or any standard OIDC provider). Off by
    default -- with NEXUS_AUTH_ENABLED=false every page behaves exactly as it
    did before this existed."""
    if settings.auth_enabled:
        if not (settings.oidc_issuer and settings.oidc_client_id and settings.oidc_client_secret):
            raise RuntimeError("NEXUS_AUTH_ENABLED=true requires NEXUS_OIDC_ISSUER, NEXUS_OIDC_CLIENT_ID and NEXUS_OIDC_CLIENT_SECRET")

        oauth = OAuth()
        oauth.register(
            name="oidc",
            server_metadata_url=f"{settings.oidc_issuer.rstrip('/')}/.well-known/openid-configuration",
            client_id=settings.oidc_client_id,
            client_secret=settings.oidc_client_secret,
            client_kwargs={"scope": "openid profile email"},
        )
        app.state.oauth_client = oauth.oidc

        @app.get("/login", include_in_schema=False)
        async def login(request: Request):
            request.session["post_login_redirect"] = _safe_next(request.query_params.get("next"))
            redirect_uri = settings.oidc_redirect_url or str(request.url_for("auth_callback"))
            return await app.state.oauth_client.authorize_redirect(request, redirect_uri)

        @app.get("/auth/callback", include_in_schema=False, name="auth_callback")
        async def auth_callback(request: Request):
            token = await app.state.oauth_client.authorize_access_token(request)
            userinfo = token.get("userinfo") or await app.state.oauth_client.userinfo(token=token)
            request.session["user"] = {
                "sub": userinfo.get("sub"),
                "email": userinfo.get("email"),
                "name": userinfo.get("name") or userinfo.get("preferred_username") or userinfo.get("email") or "Unknown",
            }
            destination = request.session.pop("post_login_redirect", "/")
            return RedirectResponse(url=destination)

        @app.get("/logout", include_in_schema=False)
        async def logout(request: Request):
            request.session.clear()
            return RedirectResponse(url="/")

        app.add_middleware(RequireLoginMiddleware)

    # Always present -- /login's CSRF state/nonce need somewhere to live even
    # for the very first request, and it's harmless when auth is disabled.
    app.add_middleware(SessionMiddleware, secret_key=ensure_session_secret(settings.session_secret_path), same_site="lax")
