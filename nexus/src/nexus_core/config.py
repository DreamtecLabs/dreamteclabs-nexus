from functools import lru_cache
from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    environment: str = Field(default="development", alias="NEXUS_ENVIRONMENT")
    data_dir: Path = Field(default=Path("/var/lib/dreamteclabs-nexus"), alias="NEXUS_DATA_DIR")
    pdm_base_url: str = Field(default="https://127.0.0.1:8443", alias="PDM_BASE_URL")
    pdm_verify_tls: bool = Field(default=True, alias="PDM_VERIFY_TLS")
    pdm_health_path: str = Field(default="/api2/json/version", alias="PDM_HEALTH_PATH")
    provider_timeout_seconds: float = Field(default=5.0, alias="NEXUS_PROVIDER_TIMEOUT_SECONDS")
    signoz_url: str = Field(default="http://192.168.0.47:8080", alias="NEXUS_SIGNOZ_URL")
    signoz_api_key: str | None = Field(default=None, alias="NEXUS_SIGNOZ_API_KEY")

    @property
    def monitoring_inventory_path(self) -> Path:
        return self.data_dir / "monitoring.json"

    @property
    def monitoring_file_sd_path(self) -> Path:
        return self.data_dir / "prometheus" / "monitoring-targets.json"


@lru_cache
def get_settings() -> Settings:
    return Settings()
