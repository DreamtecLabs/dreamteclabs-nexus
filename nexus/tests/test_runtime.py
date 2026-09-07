import json
from pathlib import Path

from fastapi.testclient import TestClient

from nexus_core.config import Settings
from nexus_core.main import create_app


def test_startup_materializes_empty_prometheus_discovery(tmp_path: Path) -> None:
    settings = Settings(
        NEXUS_DATA_DIR=tmp_path,
        PDM_BASE_URL="https://pdm.invalid",
        PDM_VERIFY_TLS=False,
    )

    with TestClient(create_app(settings)) as client:
        assert client.get("/health").status_code == 200

    discovery = settings.monitoring_file_sd_path
    assert discovery.exists()
    assert json.loads(discovery.read_text(encoding="utf-8")) == []
