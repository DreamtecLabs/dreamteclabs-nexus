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

## Initial seam

The default Dashboard route is redirected to `NexusHome`. The first `NexusHome` delegates to the upstream PDM dashboard so behavior and data loading remain identical while the ownership boundary is established. Subsequent visual changes should happen inside the Nexus namespace rather than by rewriting the PDM dashboard in place.
