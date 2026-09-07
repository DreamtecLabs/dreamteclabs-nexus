from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app


def test_health_endpoint() -> None:
    app = create_app(Settings())
    client = TestClient(app)

    response = client.get("/health")

    assert response.status_code == 200
    assert response.json() == {"status": "ok", "service": "nexus-core"}


def test_unknown_provider_returns_404() -> None:
    app = create_app(Settings())
    client = TestClient(app)

    response = client.get("/api/v1/providers/unknown/health")

    assert response.status_code == 404
