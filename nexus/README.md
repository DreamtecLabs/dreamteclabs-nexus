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

## Infrastructure and Resource Power Center

`GET /api/v1/infrastructure/resources` and `/infrastructure` expose the canonical PDM-backed Infrastructure read model. Nexus normalizes provider-native resource names into stable types such as `pve-lxc`, `pve-qemu`, `pve-node`, `pve-storage`, `pve-network`, `pbs-node` and `pbs-datastore`. PDM remains the source of truth for runtime state.

The Resource Power Center is available at `/infrastructure/power`. Mutations are **disabled by default** with `NEXUS_POWER_OPERATIONS_ENABLED=false`. When deliberately enabled, the current safe slice supports state-aware `start`, graceful `shutdown`, and immediate `stop` for PVE QEMU/LXC guests only. Hard stop requires typing the exact resource name. Nexus does not equate an accepted PDM request with completion: it reads the PDM inventory back until the expected final state is observed or returns a verification timeout. Successful and failed submitted mutations are written to the append-only `${NEXUS_DATA_DIR}/power-operations.jsonl` audit trail and exposed through `GET /api/v1/infrastructure/power/history`.

Reboot is intentionally not in this first vNext slice: a guest may remain `running` throughout a reboot, so a simple state read-back cannot prove the reboot task completed. It should be added with explicit PDM task lifecycle verification rather than a false-success shortcut.

The PDM token must have only the lifecycle permissions Nexus actually needs. A token that can read inventory but cannot mutate guests is valid for normal read-only operation; keep the power gate disabled in that case.

## Runtime deployment

The standalone runtime supports both Compose and the native systemd deployment used by `nexus-01`. `deploy.sh` chooses native deployment automatically when Docker Compose is unavailable. On a new host, copy `.env.example` to `.env`, set environment-specific values and keep secrets only in `.env`.

The native runtime installs Nexus Core into its venv, writes `nexus-core.service` and `nexus-otel.service`, restarts them and verifies health. Monitoring discovery is rebuilt from the Nexus-owned inventory on every Nexus Core startup, so collector state is not dependent on a previous API write.

Existing PDM services remain in place during migration. Deploying Nexus Core does not replace or stop PDM; production cutover happens domain-by-domain only after parity validation.

## Change policy

New Nexus features belong here, not under the upstream PDM backend/UI trees. Existing PDM customizations remain supported while they are migrated. Changes inside PDM should be limited to provider compatibility, security fixes, packaging and migration work.
