# ADR 0001: Nexus owns the control plane; PDM is a provider

- Status: Accepted
- Date: 2026-09-07

## Context

DreamtecLabs Nexus started by extending the Proxmox Datacenter Manager source tree. That gave rapid access to PDM resources and authentication, but the coupling made unrelated Nexus changes inherit PDM's Rust/WASM/Debian build cost. Small domain changes increasingly required heavyweight builds, Proxmox development repositories, packaging dependencies and long self-hosted-runner cycles.

The coupling also blurred responsibility: monitoring policy, domains/hosting orchestration and other Nexus business rules were being placed next to provider implementation code.

## Decision

DreamtecLabs Nexus is a separate control-plane application.

PDM/Proxmox is an infrastructure provider behind a port/adapter boundary. The same rule applies to SigNoz, Cloudflare, Hestia and DreamtecLabs Notify.

The target dependency direction is:

```text
Nexus UI -> Nexus API -> Domain Service -> Port -> Provider Adapter -> External system
```

Domain code must not import provider-specific clients or models. Provider adapters translate between Nexus domain contracts and external APIs.

New Nexus features must be implemented under `nexus/`. New domain behavior must not be added to `server/src/api/nexus/**` or `ui/src/nexus/**` in the PDM source tree.

Existing PDM customizations are not rewritten immediately. They remain operational while they are migrated incrementally. Changes to the PDM tree are limited to:

1. provider compatibility and API exposure required by the Nexus adapter;
2. security/bug fixes;
3. packaging/deployment changes;
4. removal or migration of existing Nexus customizations.

## CI consequence

The Nexus control plane has an independent workflow. A change confined to `nexus/**` does not compile PDM, the Rust backend, the Rust/WASM UI or Debian packages.

PDM validation remains for changes that actually touch PDM/provider code. Full package builds belong to release/post-merge validation rather than the default feedback loop for control-plane changes.

## Migration order

1. Establish Nexus Core and the PDM provider boundary.
2. Move Monitoring control-plane logic out of PDM. SigNoz and Notify become provider adapters/integrations owned by Nexus Core.
3. Move Domains & Hosting orchestration out of PDM; PDM remains only an infrastructure source/provider where needed.
4. Move Inventory and power/resource orchestration behind provider ports.
5. Move the Nexus UI out of the PDM UI tree so product UI changes no longer require a PDM WASM build.
6. Remove obsolete PDM customizations after production parity is verified.

## Consequences

Positive:

- small Nexus changes get a small CI blast radius;
- provider outages/failures are contained at adapter boundaries;
- PDM upgrades become easier because the upstream fork carries fewer product features;
- domain services become testable without a live Proxmox environment;
- additional providers can be introduced without changing domain logic.

Trade-offs:

- during migration there are two application boundaries to operate;
- authentication/session integration between the independent Nexus UI/API and PDM must be designed explicitly;
- existing features need staged migration rather than a one-time rewrite.

The migration is intentionally incremental. Production functionality is preserved until its replacement has passed parity validation.
