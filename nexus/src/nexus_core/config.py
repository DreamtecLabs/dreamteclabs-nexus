from functools import lru_cache

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    environment: str = Field(default="development", alias="NEXUS_ENVIRONMENT")
    pdm_base_url: str = Field(default="https://127.0.0.1:8443", alias="PDM_BASE_URL")
    pdm_verify_tls: bool = Field(default=True, alias="PDM_VERIFY_TLS")
    pdm_health_path: str = Field(default="/api2/json/version", alias="PDM_HEALTH_PATH")
    provider_timeout_seconds: float = Field(default=5.0, alias="NEXUS_PROVIDER_TIMEOUT_SECONDS")


@lru_cache
def get_settings() -> Settings:
    return Settings()
