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
    assert "Nexus owns target lifecycle and maintenance" in response.text

    stylesheet = client.get("/static/nexus.css")
    assert stylesheet.status_code == 200
    assert ".topbar" in stylesheet.text


def test_overview_uses_approved_compact_dashboard_shell(tmp_path: Path) -> None:
    client = _client(tmp_path)
    response = client.get("/")
    assert response.status_code == 200
    for text in (
        "Your Infrastructure. Unified.",
        "Total Resources",
        "Infrastructure Estate",
        "Recent Activity",
        "System Status",
        "Domains &amp; Hosting",
        "Upcoming &amp; Alerts",
    ):
        assert text in response.text
    assert "Operations without coupling product logic to PDM" not in response.text
    assert "WELCOME TO NEXUS" not in response.text

    dashboard_css = client.get("/static/dashboard.css")
    assert dashboard_css.status_code == 200
    assert ".kpi-grid" in dashboard_css.text
    assert ".dashboard-grid" in dashboard_css.text
    assert "background:#fff" in dashboard_css.text
