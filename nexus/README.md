# Nexus control plane

This directory is the application boundary for DreamtecLabs Nexus.

Nexus is **not** implemented as a growing set of patches inside Proxmox Datacenter Manager (PDM). PDM is an infrastructure provider. The control plane owns business/domain rules, integrations, orchestration, persistence, monitoring policy and the Nexus API.

The dependency direction is `Nexus UI -> Nexus API / Domain Services -> Ports -> Adapters`. Current adapters include PDM, SigNoz, Cloudflare/Hestia orchestration and DreamtecLabs Notify telemetry. Provider adapters may call external systems; domain services must not import provider-specific implementation details.

## Local development

Python 3.12+ is required. Create a venv, install `.[dev]`, run `pytest -q`, then `uvicorn nexus_core.main:app --reload --port 8081`.

Configuration is environment based. Production PDM access uses a dedicated least-privilege API token through `PDM_API_TOKEN_ID` and `PDM_API_TOKEN_SECRET`. Keep all secrets only in the local `.env` file and never commit them.

## Infrastructure, Resource Center and Power Center

`GET /api/v1/infrastructure/resources` and `/infrastructure` expose the canonical PDM-backed Infrastructure read model. The Resource Center enriches it with optional CPU, memory, disk and uptime fields when PDM provides them. Missing provider metrics remain unavailable rather than being fabricated. Resource detail and Estate views provide stable topology context without introducing a second discovery database.

The Resource Power Center is available at `/infrastructure/power`. Mutations are disabled by default with `NEXUS_POWER_OPERATIONS_ENABLED=false`. Supported QEMU/LXC actions are state-aware start, graceful shutdown and immediate stop. Hard stop requires the exact resource name and all submitted mutations are read back from PDM and written to `${NEXUS_DATA_DIR}/power-operations.jsonl`.

## Monitoring Control Center

`/monitoring` is the standalone vNext Monitoring Control Center. Nexus owns target intent and lifecycle; OTel consumes Nexus-generated Prometheus file discovery; SigNoz is authoritative for telemetry and planned maintenance. Enabled targets are queried through SigNoz v5 and classified healthy/down/unknown. Maintenance and disabled targets are removed from active discovery. Provider errors never expose API keys or response bodies.

Every target has a `profile`:
- `prometheus` (default) — the target exposes its own metrics endpoint; Nexus writes `${NEXUS_DATA_DIR}/prometheus/monitoring-targets.json` and the OTel Prometheus receiver scrapes `address:port/metrics_path` directly.
- `icmp` — agentless mode for resources with no exporter (a bare LXC/VM, or a network device like a Zigbee coordinator). Nexus writes `${NEXUS_DATA_DIR}/prometheus/monitoring-icmp-targets.json` with just the target address; the `prometheus/nexus_icmp` scrape job in `observability/otel-collector.yaml` relabels it to probe through the host's Blackbox Exporter (`127.0.0.1:9115`, ICMP module) instead of the target itself, reusing the exporter already deployed for the legacy PDM ICMP pipeline (`services/prometheus-blackbox-exporter-nexus.conf`). The health metric defaults to `probe_success`. **Prerequisite:** `prometheus-blackbox-exporter.service` must be installed and running on the Nexus Core host — Nexus Core only writes file discovery, it does not manage that systemd unit.

`ProvisionGuestInput.monitoring` accepts `icmp` alongside `none|pdm|prometheus`, registering an agentless probe target automatically at guest-creation time when a static address is available.

The Provisioning wizard's VMID field is populated from `ProvisioningOptions.used_vmids` (parsed from the same PDM `resources/list` call as the rest of the options) so only free VMIDs starting at PDM's suggested next ID are selectable — a used VMID can never be chosen. `ProvisionGuestInput.root_password` sets an LXC root password natively (like `ssh_public_key`, ignored for QEMU since it's ISO-installed with no cloud-init). The wizard's Template/ISO step calls `GET /api/v1/provisioning/storage-content` to list a storage's actual volumes; until the matching PDM endpoint exists server-side, that call fails and the UI falls back to manual volume-ID entry without breaking the flow.

`ProvisionGuestInput.bootstrap_otel` (LXC only, requires a static `ip_config`) installs and configures the OpenTelemetry Collector agent used across the DreamtecLabs fleet on the freshly created guest. Nexus owns a dedicated ed25519 keypair at `${NEXUS_DATA_DIR}/nexus_ssh_key`, generated once on first use (`SshBootstrapService.ensure_keypair()`); its public half is merged into the guest's `ssh-public-keys` at creation time alongside any operator-supplied key, so Nexus can reach the guest without depending on operator credentials. Once the guest is reachable over SSH, Nexus pushes and runs `services/nexus-otel-lxc-agent.sh` (read fresh from the git checkout at request time, like `services/nexus-domains-helper` — only `git pull` is needed to pick up script changes, no `./deploy.sh`), templated with `NEXUS_OTEL_AGENT_VERSION` and the existing `SIGNOZ_OTLP_ENDPOINT` setting (the same one `nexus-otel`'s Compose service already uses — kept as a single source of truth). Failure at any stage (no static address, SSH never becomes reachable, script exits non-zero) is reported as a `bootstrap-otel` warning rather than failing the whole provision.

## Domains & Hosting

`/domains` and `GET /api/v1/domains` are the standalone Domains & Hosting control plane. Nexus owns a small JSON inventory at `${NEXUS_DATA_DIR}/domains-hosting.json`, seeded with the known DreamtecLabs estate only when no Nexus inventory exists. Opening the page or listing inventory never changes Cloudflare or Hestia.

Read-only validation uses public DNS plus SMTP submission, IMAP TLS and webmail HTTPS checks. `POST /api/v1/domains/validate` is safe while mutations are locked. Cloudflare/Hestia mutation remains behind the `DomainOrchestratorProvider` and currently invokes the already validated `services/nexus-domains-helper`; the PDM Rust Domains module is not imported by Nexus Core.

Mutations are disabled by default with `NEXUS_DOMAINS_OPERATIONS_ENABLED=false`. When deliberately enabled, onboard/migrate calls the helper, persists a `pending` Nexus record, then repeats independent Nexus diagnostics. Success is reported only after all applicable post-operation checks pass; partial external success remains fail-visible as `pending`. Operations are appended to `${NEXUS_DATA_DIR}/domains-hosting-audit.jsonl`.

## Visual system

The standalone Nexus UI is light-first: white surfaces, dark readable text and colorful accents/statuses. Dark application surfaces are not part of the vNext visual baseline.

## Runtime deployment

The standalone runtime supports Compose and the native systemd deployment used by `nexus-01`. `deploy.sh` chooses native deployment when Docker Compose is unavailable. The native runtime installs Nexus Core into its venv, writes `nexus-core.service` and `nexus-otel.service`, restarts them and verifies health. Existing PDM services remain in place during migration; cutover happens domain-by-domain after parity validation.

**Compose deployment is untested and does not support Domains & Hosting.** Production (`nexus-01`) runs native-only. The `nexus-core` container passes every `.env` value through (`env_file:`), so config drift is no longer a risk there, but the container still has no `ssh` client, no host SSH keys, and no mount for `services/nexus-domains-helper` — anything that reconciles a domain (onboard/migrate) will fail. Fix all three before relying on Compose for a domain that needs onboarding.

## Change policy

New Nexus features belong under `nexus/`, not in upstream PDM backend/UI trees. Existing PDM customizations remain supported while migrated. Changes inside PDM are limited to provider compatibility, security, packaging and migration work.
