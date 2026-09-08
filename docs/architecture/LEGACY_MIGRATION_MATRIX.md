# Legacy Nexus capability migration matrix

Source reference: `DreamtecLabs/dreamteclabs-nexus-legacy` (`main`).

This document classifies legacy capabilities before implementation work begins. It is intentionally about product behavior and ownership, not copying files.

| Legacy capability | Decision | vNext owner | Provider / source | Notes |
|---|---|---|---|---|
| Dashboard / operational overview | Redesign | Platform + UI | Nexus domain APIs | Recover useful overview concepts; do not reuse NiceGUI page coupling. |
| Assets / inventory | Migrate | Infrastructure | PDM | **Implemented:** canonical PDM-backed read model and standalone searchable Inventory workspace are production-validated. PDM is authoritative for Proxmox/PBS runtime state; Nexus may persist annotations or business metadata only. |
| Resource detail / relationships | Redesign | Infrastructure | PDM + Nexus metadata | Preserve useful resource-centric UX; rebuild relationship model around stable provider identities. |
| Resource Power Center | Migrate | Infrastructure | PDM | **Implemented, gated:** vNext supports state-aware start, graceful shutdown and hard stop for QEMU/LXC through the PDM port, with configuration gating, hard-stop confirmation, post-mutation read-back and durable operation audit. Reboot remains deferred until task lifecycle verification is modeled. |
| Direct Proxmox providers / `proxmoxer` paths | Replace by provider | Infrastructure | PDM | Do not migrate direct cluster-specific access into vNext. |
| Infrastructure graph | Redesign | Infrastructure | PDM + Nexus metadata | Build only after inventory/resource identities are stable. |
| Discovery | Replace by provider / Redesign | Infrastructure | PDM first | Avoid a second broad discovery engine where PDM already knows the estate. Add non-PDM discovery only for explicit gaps. |
| Monitoring center | Migrate | Observability | SigNoz | **Implemented:** standalone Monitoring Control Center exposes SigNoz-backed live health, provider readiness, lifecycle controls and per-target health metrics. |
| Prometheus target ownership | Redesign | Observability | Nexus target intent + OTel/SigNoz | **Implemented:** Nexus target inventory/file_sd remains the single target-intent path; only enabled targets are published and stable Nexus labels are attached. |
| Alertmanager provider / silence reconciler | Replace by provider | Observability | SigNoz planned maintenance | **Implemented for maintenance:** target Maintenance/Resume uses SigNoz planned-maintenance schedules; no second Alertmanager/silence control plane is carried forward. |
| Blackbox exporter management | Redesign | Observability | SigNoz/OTel where required | Add probe-style monitoring only when a concrete service requires it. |
| Monitoring monkey patches / embedded/navigation patches | Discard | — | — | These are compatibility debt, not product capability. |
| IPAM | Redesign | Infrastructure / Networking | provider to be selected | Recover UX and use cases only after defining an authoritative IPAM source. Do not migrate ambiguous ownership. |
| Network overview/services | Redesign | Infrastructure / Networking | PDM + Cloudflare + future network provider | Separate topology/configuration from service reachability. |
| Domains / DNS lifecycle | Migrate | Domains & Hosting | Cloudflare | Reuse validated domain rules conceptually; implement behind Cloudflare port. |
| Website / hosting management | Migrate | Domains & Hosting | Hestia | Provider mutations must be verified before success is reported. |
| Mail provisioning | Migrate | Domains & Hosting | Hestia | Keep as a domain workflow, not direct UI-to-Hestia calls. |
| Cloudflare Tunnel publishing | Migrate | Domains & Hosting | Cloudflare | Preserve safe remote-managed tunnel semantics where still applicable. |
| Applications / services catalog | Redesign | Platform / Infrastructure | Nexus metadata + providers | Recover service-centric UX, but normalize identity before adding broad automation. |
| Backups | Redesign | Infrastructure | PDM/PBS | Prefer PDM/PBS provider state instead of maintaining independent backup discovery where possible. |
| Compliance | Defer / Redesign | Platform | multiple | Useful later, but not a foundation dependency. |
| Engineering / product intelligence | Defer / Redesign | Platform | multiple | Product concept may return after core operational domains are stable. |
| Product manifests | Defer | Platform | GitHub / metadata | Do not include in initial convergence scope. |
| Secrets vault | Redesign | Platform | dedicated secret mechanism | Do not copy legacy secret-storage behavior without a separate security review. |
| Collector engine / multiple collector services | Replace / Minimize | Observability | OTel + explicit adapters | Prefer one observability collection path. |
| Edge agent | Defer / Selective migrate | Platform / Networking | edge agent | Keep only capabilities not already owned by Cloudflare/PDM/SigNoz. |
| Docker-specific control plane | Defer | Infrastructure | future provider | Not part of initial bounded contexts. |
| Bootstrap / provisioning workflows | Redesign | Infrastructure | PDM + explicit providers | Reintroduce only after lifecycle contracts are stable. |
| Legacy UI (`app/ui`) | Discard implementation | UI | — | Use as functional reference only. |
| Legacy UI v2 (`app/ui_v2`) | Discard implementation | UI | — | Recover navigation and workflows selectively, not compatibility code. |
| NiceGUI runtime coupling | Discard | UI | — | vNext stays server-rendered/lightweight and independent of provider stateful UI sessions. |
| Legacy composition root startup jobs/workers | Redesign | Platform | explicit services | Each background job must have one owner and isolated failure behavior. |
| Platform health | Migrate | Platform | Nexus + provider health | Keep fail-visible health semantics without making unrelated provider outages crash startup. |
| Audit / operation history | Redesign and prioritize | Platform | Nexus persistence | **Started:** Infrastructure power mutations now write a durable append-only JSONL audit and expose recent history. Move to the future Nexus DB audit model when the Platform persistence layer is introduced. |

## Migration order

1. Foundation and enforceable architecture boundaries. **Done.**
2. Infrastructure inventory/resource identities through PDM. **Done and production validated.**
3. Infrastructure power/lifecycle operations through PDM. **Implemented behind an explicit production safety gate.**
4. Observability read model through SigNoz and planned maintenance. **Implemented; production deployment validation pending.** Alert-rule authoring remains a separate follow-up because Nexus must not guess provider contracts or notification-channel policy.
5. Resource-centric UI expansion using domain APIs only.
6. Domains & Hosting through Cloudflare/Hestia.
7. Applications/services, backups and networking/IPAM after authoritative data ownership is decided.
8. Deferred capabilities only after the core control plane has demonstrated production stability.

## Explicit non-goals

The convergence does not aim for file-level parity with the legacy repository. It does not preserve old URLs, monkey patches, UI generations, direct Proxmox access or every historical module. Production parity is defined by agreed user workflows and operational outcomes, not by matching the old code structure.
