# ADR 0003 — Guest provisioning stays behind PDM

## Status

Accepted for Nexus vNext provisioning.

## Context

Nexus needs a Proxmox-like LXC/VM creation workflow, but the vNext architecture does not allow Nexus domain code to connect directly to individual PVE clusters. PDM is the authoritative provider boundary for Proxmox estates.

PDM already exposes inventory, node, storage, network, next-VMID and guest lifecycle APIs. Its current API did not expose collection-level guest creation, even though its remote client can issue authenticated PVE API requests.

## Decision

PDM gains explicit node-scoped `create-lxc` and `create-qemu` provider endpoints. They require PDM `Resource.Manage` permission and forward a native PVE creation parameter object through PDM's configured remote client. Nexus calls only those PDM endpoints.

Nexus owns the orchestration around creation:

- preflight validation and duplicate VMID/name checks;
- Proxmox-style wizard and confirmation;
- PDM create request;
- resource read-back verification through the existing canonical inventory;
- optional LXC SSH public-key injection;
- monitoring selection and Prometheus target registration where an endpoint exists;
- explicit ready / ready-with-warnings outcomes.

Provisioning is disabled by default with `NEXUS_PROVISIONING_ENABLED=false`.

## Consequences

Nexus remains portable away from the PDM host and never needs per-cluster PVE credentials. PDM remains the sole owner of PVE connectivity and permission enforcement. Native PVE create parameters can evolve without introducing a second Proxmox client into Nexus.

A successfully created guest is never automatically deleted because a later Nexus setup step fails. Post-create failures are surfaced as warnings and can be retried in future workflow iterations.
