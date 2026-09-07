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

Configuration is environment based. The PDM adapter starts with `PDM_BASE_URL=https://127.0.0.1:8443`; no credentials are stored in this repository.

## Change policy

New Nexus features belong here, not under the upstream PDM backend/UI trees. Existing PDM customizations remain supported while they are migrated. Changes inside PDM should be limited to provider compatibility, security fixes, packaging and migration work.
