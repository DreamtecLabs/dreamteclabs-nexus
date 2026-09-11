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
    pdm_api_token_id: str | None = Field(default=None, alias="PDM_API_TOKEN_ID")
    pdm_api_token_secret: str | None = Field(default=None, alias="PDM_API_TOKEN_SECRET")
    provider_timeout_seconds: float = Field(default=5.0, alias="NEXUS_PROVIDER_TIMEOUT_SECONDS")
    power_operations_enabled: bool = Field(default=False, alias="NEXUS_POWER_OPERATIONS_ENABLED")
    power_verification_attempts: int = Field(default=60, ge=1, le=120, alias="NEXUS_POWER_VERIFY_ATTEMPTS")
    power_verification_interval_seconds: float = Field(default=1.0, ge=0.1, le=10.0, alias="NEXUS_POWER_VERIFY_INTERVAL_SECONDS")
    decommission_enabled: bool = Field(default=False, alias="NEXUS_DECOMMISSION_ENABLED")
    provisioning_enabled: bool = Field(default=False, alias="NEXUS_PROVISIONING_ENABLED")
    provisioning_verification_attempts: int = Field(default=90, ge=1, le=300, alias="NEXUS_PROVISIONING_VERIFY_ATTEMPTS")
    provisioning_verification_interval_seconds: float = Field(default=1.0, ge=0.1, le=10.0, alias="NEXUS_PROVISIONING_VERIFY_INTERVAL_SECONDS")
    signoz_url: str = Field(default="http://192.168.0.47:8080", alias="NEXUS_SIGNOZ_URL")
    signoz_api_key: str | None = Field(default=None, alias="NEXUS_SIGNOZ_API_KEY")
    cloudflare_api_base: str = Field(default="https://api.cloudflare.com/client/v4", alias="NEXUS_CF_API_BASE")
    cloudflare_api_token: str | None = Field(default=None, alias="NEXUS_CF_API_TOKEN")
    cloudflare_account_id: str | None = Field(default=None, alias="NEXUS_CF_ACCOUNT_ID")
    cloudflare_tunnel_id: str | None = Field(default=None, alias="NEXUS_CF_TUNNEL_ID")
    domains_operations_enabled: bool = Field(default=False, alias="NEXUS_DOMAINS_OPERATIONS_ENABLED")
    domains_helper_path: Path = Field(default=Path("/opt/dreamteclabs-nexus/services/nexus-domains-helper"), alias="NEXUS_DOMAINS_HELPER_PATH")
    domains_helper_timeout_seconds: float = Field(default=300.0, ge=10, le=900, alias="NEXUS_DOMAINS_HELPER_TIMEOUT_SECONDS")
    domains_verification_attempts: int = Field(default=3, ge=1, le=20, alias="NEXUS_DOMAINS_VERIFY_ATTEMPTS")
    domains_verification_interval_seconds: float = Field(default=3.0, ge=0.1, le=30, alias="NEXUS_DOMAINS_VERIFY_INTERVAL_SECONDS")
    signoz_otlp_endpoint: str = Field(default="192.168.0.47:4317", alias="SIGNOZ_OTLP_ENDPOINT")
    otel_agent_version: str = Field(default="0.139.0", alias="NEXUS_OTEL_AGENT_VERSION")
    otel_agent_script_path: Path = Field(default=Path("/opt/dreamteclabs-nexus/services/nexus-otel-lxc-agent.sh"), alias="NEXUS_OTEL_AGENT_SCRIPT_PATH")
    ssh_bootstrap_attempts: int = Field(default=30, ge=1, le=120, alias="NEXUS_SSH_BOOTSTRAP_ATTEMPTS")
    ssh_bootstrap_interval_seconds: float = Field(default=5.0, ge=0.5, le=30, alias="NEXUS_SSH_BOOTSTRAP_INTERVAL_SECONDS")
    ssh_bootstrap_run_timeout_seconds: float = Field(default=180.0, ge=10, le=900, alias="NEXUS_SSH_BOOTSTRAP_RUN_TIMEOUT_SECONDS")

    @property
    def ssh_bootstrap_key_path(self) -> Path:
        return self.data_dir / "nexus_ssh_key"

    @property
    def monitoring_inventory_path(self) -> Path:
        return self.data_dir / "monitoring.json"

    @property
    def monitoring_file_sd_path(self) -> Path:
        return self.data_dir / "prometheus" / "monitoring-targets.json"

    @property
    def monitoring_icmp_file_sd_path(self) -> Path:
        return self.data_dir / "prometheus" / "monitoring-icmp-targets.json"

    @property
    def power_audit_path(self) -> Path:
        return self.data_dir / "power-operations.jsonl"

    @property
    def domains_inventory_path(self) -> Path:
        return self.data_dir / "domains-hosting.json"

    @property
    def domains_audit_path(self) -> Path:
        return self.data_dir / "domains-hosting-audit.jsonl"


@lru_cache
def get_settings() -> Settings:
    return Settings()
