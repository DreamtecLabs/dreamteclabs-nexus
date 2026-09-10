from pathlib import Path

from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app


def _client(tmp_path: Path) -> TestClient:
    app = create_app(
        Settings(
            NEXUS_DATA_DIR=tmp_path,
            PDM_BASE_URL="https://pdm.invalid",
            PDM_VERIFY_TLS=False,
            NEXUS_PROVIDER_TIMEOUT_SECONDS=0.01,
        )
    )
    return TestClient(app)


def test_standalone_monitoring_ui_is_served_by_nexus_core(tmp_path: Path) -> None:
    client = _client(tmp_path)
    response = client.get("/monitoring")
    assert response.status_code == 200
    assert "Monitoring Control Center" in response.text
    assert 'class="rail"' in response.text
    assert 'class="rail-link active"' in response.text
    assert 'href="/monitoring"' in response.text

    stylesheet = client.get("/static/nexus.css")
    assert stylesheet.status_code == 200
    assert ".topbar" in stylesheet.text
    assert ".rail{" in stylesheet.text


def test_overview_uses_approved_console_shell(tmp_path: Path) -> None:
    client = _client(tmp_path)
    response = client.get("/")
    assert response.status_code == 200
    for text in (
        "Your infrastructure, unified",
        "Resources",
        "Infrastructure estate",
        "Recent activity",
        "System status",
        "Domains & hosting",
        "Upcoming & alerts",
    ):
        assert text in response.text
    assert "Operations without coupling product logic to PDM" not in response.text
    assert "WELCOME TO NEXUS" not in response.text
    assert 'href="/static/dashboard.css"' in response.text

    dashboard_css = client.get("/static/dashboard.css")
    assert dashboard_css.status_code == 200
    assert ".kpi-row" in dashboard_css.text
    assert ".grid-2col" in dashboard_css.text

    nexus_css = client.get("/static/nexus.css")
    assert nexus_css.status_code == 200
    # Light palette is the unconditional :root default; dark only applies under
    # prefers-color-scheme or an explicit data-theme opt-in, never by default.
    assert "--bg:#F3F4F9" in nexus_css.text
    assert "prefers-color-scheme: dark" in nexus_css.text
