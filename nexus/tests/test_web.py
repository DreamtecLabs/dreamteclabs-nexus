from pathlib import Path

from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app


def test_standalone_monitoring_ui_is_served_by_nexus_core(tmp_path: Path) -> None:
    app = create_app(
        Settings(
            NEXUS_DATA_DIR=tmp_path,
            PDM_BASE_URL="https://pdm.invalid",
            PDM_VERIFY_TLS=False,
        )
    )
    client = TestClient(app)

    response = client.get("/monitoring")
    assert response.status_code == 200
    assert "Monitoring Control Center" in response.text
    assert "Nexus owns target lifecycle and maintenance" in response.text

    stylesheet = client.get("/static/nexus.css")
    assert stylesheet.status_code == 200
    assert ".topbar" in stylesheet.text
