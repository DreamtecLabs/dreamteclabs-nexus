# SigNoz integration contract

Nexus treats SigNoz as the observability backend, not as inventory. Device/host identity and monitoring intent remain Nexus-owned.

The backend uses the SigNoz HTTP API with a service-account API key supplied through `NEXUS_SIGNOZ_API_KEY`. The current self-hosted endpoint defaults to `http://192.168.0.47:8080` and can be overridden with `NEXUS_SIGNOZ_URL`.

The initial implementation performs a real authenticated API call to the alert-rules endpoint to verify connectivity and report a rule count. Alert-rule creation/update will be added only after the exact rule schema exposed by the deployed SigNoz version is validated; Nexus must not guess or hard-code an unverified alert payload.

ICMP telemetry is sent directly from the dedicated OpenTelemetry collector to the current OTLP gRPC endpoint `192.168.0.47:4317`.
