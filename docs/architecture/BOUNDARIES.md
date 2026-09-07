# Nexus architecture boundaries

`nexus/` is the DreamtecLabs Nexus product and control plane. Product domains, orchestration, policies, persistence, presentation, and integrations belong there.

The upstream Proxmox Datacenter Manager source tree is not the Nexus application framework. PDM/Proxmox is an infrastructure provider consumed through ports/adapters. The same rule applies to other systems such as SigNoz, Cloudflare, Hestia, and DreamtecLabs Notify: integrations implement provider interfaces; domain services do not depend directly on vendor-specific APIs.

## Placement rules

- New product/domain code belongs under `nexus/src/nexus_core/`.
- New product UI belongs under the standalone Nexus web application, not `ui/src/nexus/`.
- PDM-specific transport/auth/resource mapping belongs in a PDM provider adapter.
- Existing files under `server/src/api/nexus/` and `ui/src/nexus/` are legacy migration surfaces. They may be changed only to preserve production parity, adapt provider behavior, migrate functionality, or remove obsolete code.
- No new files may be added to those legacy PDM Nexus paths. CI enforces this rule.
- Legacy functionality is removed only after the corresponding Nexus Core path is deployed and parity has been validated.

## Dependency direction

UI/API -> Domain Service -> Port -> Provider/Repository adapter

Domain services must be testable with in-memory/fake adapters and must not require a running PDM, SigNoz, Cloudflare, Hestia, or other external service for unit tests.

## CI rule

Changes inside `nexus/**` use the lightweight Nexus Core workflow and must not trigger PDM Rust/WASM/Debian builds. Provider-specific PDM changes continue to use PDM validation. Full PDM packaging is a provider/release concern rather than the default validation path for Nexus product changes.
