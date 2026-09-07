from pathlib import Path


def test_native_deploy_bootstraps_venv_on_debian() -> None:
    script = Path("deploy-native.sh").read_text()
    assert "apt-get install -y python3-venv" in script
    assert "rm -rf .venv" in script
    assert "create_venv" in script
