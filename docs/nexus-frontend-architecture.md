# Nexus frontend architecture

DreamtecLabs Nexus is maintained as an upstream-friendly frontend layer on top of Proxmox Datacenter Manager (PDM).

## Boundary

- Keep the PDM backend, API, resource model, authentication, remotes and task engine unchanged whenever possible.
- Keep Nexus-owned presentation code under `ui/src/nexus/`.
- Limit edits to upstream UI files to small integration seams.
- Do not duplicate PDM API calls or resource models when an upstream client/model already exists.

## Upstream synchronization goal

A PDM update should normally affect Nexus in one of two ways:

1. no conflict because the change is outside Nexus-owned frontend code; or
2. a small, explicit conflict at an integration seam such as navigation or routing.

Large edits to PDM core UI files are considered an architectural regression and should be refactored into the Nexus namespace before merge.

## Current integration seam

The default Dashboard route is redirected to `NexusHome`. `NexusHome` owns the Nexus dashboard composition and visual system while consuming PDM's existing `/resources/status` API and data types. The PDM backend, resource model, authentication and operational engine remain unchanged.

Primary navigation and top-level branding are adapted through small UI integration seams in `ui/src/main_menu.rs` and `ui/src/top_nav_bar.rs`. New Nexus presentation work should continue under `ui/src/nexus/` rather than rewriting PDM backend or domain logic.
