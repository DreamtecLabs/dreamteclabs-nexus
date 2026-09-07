# Nexus control plane

This directory is the application boundary for DreamtecLabs Nexus.

Nexus is **not** implemented as a growing set of patches inside Proxmox Datacenter Manager (PDM). PDM is an infrastructure provider. The control plane owns business/domain rules, integrations, orchestration, persistence, monitoring policy and the Nexus API.

The dependency direction is:

```text
Nexus UI
   |
Nexus API / Domain Services
   |
Ports (provider/repository interfaces)
   |
Adapters
   +-- PDM / Proxmox
   +-- SigNoz
   +-- Cloudflare
   +-- Hestia
   +-- DreamtecLabs Notify
```

Provider adapters may call external systems. Domain services must not import provider-specific implementation details.

## Local development

Python 3.12+ is required.

```bash
python -m venv .venv
. .venv/bin/activate
python -m pip install -e '.[dev]'
pytest -q
uvicorn nexus_core.main:app --reload --port 8081
```

Configuration is environment based. The PDM adapter starts with `PDM_BASE_URL=https://127.0.0.1:8443`; no credentials are stored in this repository. Production PDM access should use a dedicated least-privilege API token supplied through `PDM_API_TOKEN_ID` and `PDM_API_TOKEN_SECRET`. The adapter sends it using PDM's `PDMAPIToken TOKENID:TOKENSECRET` authorization scheme. Keep the secret only in the local `.env` file and never commit it.

## Runtime deployment

The standalone runtime is `compose.yaml`: `nexus-core` serves the API/UI on host port 8081 and `nexus-otel` discovers Nexus-managed Prometheus targets through file discovery and exports metrics to SigNoz.

On a new host, copy `.env.example` to `.env`, set environment-specific values and keep secrets only in `.env`. `deploy.sh` validates Compose, pulls the pinned OpenTelemetry Collector, builds Nexus Core, starts both services, waits for the container healthcheck and fails with recent logs if startup does not become healthy. Monitoring discovery is rebuilt from the Nexus-owned inventory on every Nexus Core startup, so collector state is not dependent on a previous API write.

Existing PDM services remain in place during migration. Deploying Nexus Core does not replace or stop PDM; production cutover happens domain-by-domain only after parity validation.

## Change policy

New Nexus features belong here, not under the upstream PDM backend/UI trees. Existing PDM customizations remain supported while they are migrated. Changes inside PDM should be limited to provider compatibility, security fixes, packaging and migration work.
