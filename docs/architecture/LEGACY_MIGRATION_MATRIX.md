# Legacy Nexus capability migration matrix

Source reference: `DreamtecLabs/dreamteclabs-nexus-legacy` (`main`).

This document classifies legacy capabilities before implementation work begins. It is intentionally about product behavior and ownership, not copying files.

| Legacy capability | Decision | vNext owner | Provider / source | Notes |
|---|---|---|---|---|
| Dashboard / operational overview | Redesign | Platform + UI | Nexus domain APIs | Recover useful overview concepts; do not reuse NiceGUI page coupling. |
| Assets / inventory | Migrate | Infrastructure | PDM | **Implemented:** canonical PDM-backed read model and standalone searchable Inventory workspace are production-validated. PDM is authoritative for Proxmox/PBS runtime state. |
| Resource detail / relationships | Redesign | Infrastructure | PDM + Nexus metadata | **Implemented from provider state:** Resource Center exposes per-resource telemetry and node/guest/storage/network relationships using stable PDM identities. |
| Resource Power Center | Migrate | Infrastructure | PDM | **Implemented, gated:** state-aware start, graceful shutdown and hard stop with confirmation, post-mutation read-back and durable audit. |
| Direct Proxmox providers / `proxmoxer` paths | Replace by provider | Infrastructure | PDM | Do not migrate direct cluster-specific access into vNext. |
| Infrastructure graph | Redesign | Infrastructure | PDM + Nexus metadata | **Started safely:** Estate view provides deterministic remote/node topology from PDM without a graph database. |
| Discovery | Replace by provider / Redesign | Infrastructure | PDM first | Avoid a second broad discovery engine where PDM already knows the estate. |
| Monitoring center | Migrate | Observability | SigNoz | **Implemented:** standalone Monitoring Control Center exposes SigNoz-backed live health, provider readiness and lifecycle controls. |
| Prometheus target ownership | Redesign | Observability | Nexus + OTel/SigNoz | **Implemented:** Nexus target inventory/file_sd remains the single target-intent path. |
| Alertmanager provider / silence reconciler | Replace by provider | Observability | SigNoz planned maintenance | **Implemented for maintenance:** no second silence control plane. |
| Blackbox exporter management | Redesign | Observability | SigNoz/OTel | Add probe-style monitoring only for concrete needs. |
| Monitoring monkey patches / embedded/navigation patches | Discard | — | — | Compatibility debt, not product capability. |
| IPAM | Redesign | Infrastructure / Networking | provider TBD | Wait for authoritative ownership. |
| Network overview/services | Redesign | Infrastructure / Networking | PDM + Cloudflare + future provider | Separate topology/configuration from reachability. |
| Domains / DNS lifecycle | Migrate | Domains & Hosting | Cloudflare | **Implemented as vNext control-plane slice:** Nexus-owned inventory, public DNS validation and gated Cloudflare/Hestia orchestration. |
| Website / hosting management | Migrate | Domains & Hosting | Hestia | **Foundation migrated:** Hestia stays behind the orchestrator provider; website-specific lifecycle can expand later. |
| Mail provisioning | Migrate | Domains & Hosting | Hestia | **Implemented for mail/webmail domain onboarding and migration with independent post-operation validation.** |
| Cloudflare Tunnel publishing | Migrate | Domains & Hosting | Cloudflare | **Implemented for webmail publishing through the validated helper/provider path.** |
| Applications / services catalog | Redesign | Platform / Infrastructure | Nexus metadata + providers | Next major candidate after Domains & Hosting production validation. |
| Backups | Redesign | Infrastructure | PDM/PBS | Prefer PDM/PBS provider state. |
| Compliance | Defer / Redesign | Platform | multiple | Later. |
| Engineering / product intelligence | Defer / Redesign | Platform | multiple | Later. |
| Product manifests | Defer | Platform | GitHub / metadata | Later. |
| Secrets vault | Redesign | Platform | dedicated mechanism | Separate security review required. |
| Collector engine / multiple collector services | Replace / Minimize | Observability | OTel + explicit adapters | Prefer one collection path. |
| Edge agent | Defer / Selective migrate | Platform / Networking | edge agent | Only explicit gaps. |
| Docker-specific control plane | Defer | Infrastructure | future provider | Not part of initial bounded contexts. |
| Bootstrap / provisioning workflows | Redesign | Infrastructure | PDM + providers | Reintroduce only after lifecycle contracts stabilize. |
| Legacy UI / UI v2 | Discard implementation | UI | — | Functional reference only. |
| NiceGUI runtime coupling | Discard | UI | — | vNext stays lightweight and server-rendered. |
| Legacy composition root startup jobs/workers | Redesign | Platform | explicit services | Each background job gets one owner and isolated failure behavior. |
| Platform health | Migrate | Platform | Nexus + provider health | Keep fail-visible semantics. |
| Audit / operation history | Redesign and prioritize | Platform | Nexus persistence | Infrastructure and Domains now have durable append-only audit; consolidate later into Nexus DB audit. |

## Migration order

1. Foundation and enforceable architecture boundaries — **done**.
2. Infrastructure inventory/resource identities through PDM — **done and production validated**.
3. Infrastructure power/lifecycle operations through PDM — **implemented behind a production safety gate**.
4. Observability read model through SigNoz and planned maintenance — **implemented**.
5. Resource-centric UI expansion using domain APIs only — **implemented**.
6. Domains & Hosting through Cloudflare/Hestia — **implemented as a gated standalone vNext slice; production deployment validation pending**.
7. Applications/services, backups and networking/IPAM after authoritative data ownership is decided.
8. Deferred capabilities only after the core control plane has demonstrated production stability.

## Explicit non-goals

The convergence does not aim for file-level parity with the legacy repository. It does not preserve old URLs, monkey patches, UI generations, direct Proxmox access or every historical module. Production parity is defined by agreed user workflows and operational outcomes, not by matching the old code structure.
