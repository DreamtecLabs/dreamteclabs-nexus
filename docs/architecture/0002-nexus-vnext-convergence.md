# ADR 0002: Nexus vNext convergence from the legacy product

Status: Accepted

## Context

DreamtecLabs has two useful sources of Nexus functionality:

- `DreamtecLabs/dreamteclabs-nexus`, the current independent Nexus Core and the canonical product repository.
- `DreamtecLabs/dreamteclabs-nexus-legacy`, the previous Python/NiceGUI control plane with broad product coverage but substantial coupling, parallel UI generations, compatibility adapters and overlapping control paths.

The legacy product contains valuable workflows and domain knowledge, but restoring it wholesale would also restore the architectural conditions that made changes brittle. The current Nexus Core has a smaller, tested runtime and already has validated provider connectivity to PDM and SigNoz.

## Decision

The current `dreamteclabs-nexus` remains the canonical Nexus product. The legacy repository is a reference implementation and functional specification only. Capabilities are migrated selectively; directories or compatibility layers are never merged wholesale.

The required dependency direction for product code is:

```text
UI / HTTP API -> Domain Service -> Port -> Provider Adapter -> External System
```

The initial bounded contexts are:

1. **Infrastructure** — inventory, hosts, guests, resource detail and lifecycle/power operations. PDM is the authoritative provider for Proxmox estate state and mutations.
2. **Observability** — telemetry status, metric queries, alerts and planned maintenance. SigNoz owns telemetry storage/query, alert evaluation and maintenance suppression.
3. **Domains & Hosting** — DNS, published services, websites and mail. Cloudflare and Hestia are provider adapters.
4. **Platform** — Nexus health, configuration, audit and internal operational state.

The UI is a Nexus-owned product surface. It may recover useful information architecture and workflows from the legacy UI, but not NiceGUI compatibility layers, monkey patches, duplicate page generations or provider calls from presentation code.

## Provider ownership

### PDM

PDM is a provider, not the Nexus application framework. Nexus talks to PDM through its authenticated public API. Nexus code must not import PDM Rust/WASM internals. Deploying Nexus and PDM on the same host is an operational choice and must not create an in-process dependency.

### SigNoz

SigNoz is the observability system of record for telemetry queries, alert rules and planned maintenance. Nexus may own target intent and operational policy, but must not recreate a second alert manager or telemetry database.

### Cloudflare and Hestia

Cloudflare and Hestia remain external providers behind Nexus domain ports. Mutations must be verified by reading provider state back before Nexus reports success where the provider supports verification.

## Legacy migration rules

A legacy capability must be classified as one of:

- **Migrate** — product behavior remains valuable and is rebuilt behind current domain/port boundaries.
- **Redesign** — user goal remains valuable but the old implementation or information model is unsuitable.
- **Replace by provider** — capability is delegated to PDM, SigNoz, Cloudflare, Hestia or another authoritative system.
- **Discard** — compatibility code, duplicate control paths or obsolete functionality is intentionally not carried forward.

No migration is considered complete until its replacement has isolated tests and production parity is explicitly verified.

## Guardrails

- UI and HTTP route modules do not import provider implementations.
- Domain/service modules do not import provider implementations or UI frameworks.
- Port modules do not depend on providers, repositories, services or UI frameworks.
- Provider adapters implement behavior defined by ports and do not own product policy.
- The application composition root is the only place that wires concrete providers into domain services.
- New Nexus features are developed under the independent `nexus/` runtime and use fast isolated CI.

## Consequences

This approach preserves the current working Nexus Core, PDM API-token authentication, standalone runtime and SigNoz connectivity while allowing the broad product experience of the legacy Nexus to be recovered incrementally. It deliberately trades a one-time selective migration effort for a much smaller regression surface and clearer ownership boundaries.