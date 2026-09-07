from pathlib import Path


def test_native_deploy_rebuilds_incomplete_venv_without_pip() -> None:
    script = Path("deploy-native.sh").read_text()
    assert '[[ ! -x .venv/bin/python || ! -x .venv/bin/pip ]]' in script
    assert "create_venv" in script
