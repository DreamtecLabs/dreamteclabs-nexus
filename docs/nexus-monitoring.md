# Nexus Monitoring

Nexus owns the monitoring control plane. SigNoz remains the observability backend and OpenTelemetry remains the telemetry transport/runtime.

## Responsibilities

- **Nexus UI/API**: source of truth for monitored devices, monitoring state and provider configuration.
- **SigNoz**: metrics exploration, dashboards, alert rules, planned maintenance and notification routing.
- **OpenTelemetry Collector**: host metrics for Linux guests and centralized ICMP synthetic checks for agentless devices.

Operators should not edit the ICMP collector YAML to register devices. Add, remove or change devices through **Monitoring** in the Nexus UI. The backend regenerates and validates the collector configuration and reconciles the dedicated systemd service.

## Device inventory

The backend persists Nexus-owned state in `monitoring.json` under the PDM configuration directory. Device records contain:

- stable Nexus id
- human-readable name
- IPv4, IPv6 or hostname
- device kind
- site
- monitoring profile (`icmp` in the initial implementation)
- state: `enabled`, `maintenance` or `disabled`

Only `enabled` devices are rendered into the active ICMP collector configuration. Maintenance and disabled devices are intentionally excluded from probes.

## ICMP probe engine

The backend generates `/etc/proxmox-datacenter-manager/nexus-icmp-collector.yaml` for the packaged `nexus-icmp-collector.service`. Each enabled device receives an isolated `icmpcheck/<device-id>` receiver and metrics pipeline, allowing Nexus resource attributes to remain specific to that device.

The collector exports OTLP metrics to `192.168.0.47:4317` by default. The OpenTelemetry `icmpcheckreceiver` provides packet-loss and RTT metrics plus `net.peer.ip` and `net.peer.name` resource attributes.

The collector binary is intentionally not silently installed by a device CRUD operation. If `/usr/bin/otelcol-contrib` is absent, the device remains safely persisted and the UI surfaces the reconciliation failure.

## SigNoz API

The Nexus backend reads:

- `NEXUS_SIGNOZ_URL` (defaults to `http://192.168.0.47:8080`)
- `NEXUS_SIGNOZ_API_KEY` (required for authenticated SigNoz API calls)

The key stays server-side and is never returned to the browser. Nexus uses the current SigNoz rules API at `/api/v2/rules` for rule discovery and the planned-maintenance API at `/api/v1/downtime_schedules` for downtime lifecycle operations.

SigNoz remains authoritative for alert definitions. Nexus does not copy rule bodies into its own inventory. Planned maintenance created through Nexus is sent directly to SigNoz with a fixed schedule, optional alert rule IDs and an optional SigNoz label-scope expression. This preserves SigNoz evaluation while suppressing matching notifications during the maintenance window.

The backend deliberately validates only Nexus-owned input boundaries and lets SigNoz validate RFC3339 timestamps, IANA timezones and scope-expression semantics. Provider error bodies and HTTP statuses are propagated back to the operator instead of being hidden.

## API routes

- `GET /api2/json/monitoring` — inventory and local probe-engine status
- `GET /api2/json/monitoring/signoz` — safe SigNoz API connectivity summary
- `GET /api2/json/monitoring/signoz-rules` — current SigNoz alert rules
- `GET /api2/json/monitoring/signoz-downtimes` — current SigNoz planned-maintenance windows
- `POST /api2/json/monitoring/signoz-downtime` — create a fixed planned-maintenance window
- `POST /api2/json/monitoring/signoz-downtime-delete` — delete a planned-maintenance window
- `POST /api2/json/monitoring/device` — create/update a device and reconcile
- `POST /api2/json/monitoring/device-delete` — delete a device and reconcile
- `POST /api2/json/monitoring/device-probe` — one-shot diagnostic ping
- `POST /api2/json/monitoring/reconcile` — regenerate/restart the ICMP probe engine

Read operations require system audit privileges. Mutating operations require system modify privileges.

## Maintenance safety boundary

Nexus does not yet synthesize a host-specific SigNoz scope expression when an inventory device is switched to the local `maintenance` state. The exact alert-label identity emitted by each monitoring profile must first be validated in the deployed SigNoz instance. Until that validation is complete, local device maintenance stops Nexus ICMP probing and SigNoz downtime creation remains an explicit operation with rule IDs and/or scope supplied by the operator. This avoids accidentally silencing unrelated hosts.

## Next increments

The data model is designed to expand to HTTP/HTTPS, TCP and SNMP profiles without making SigNoz the inventory source of truth. The next SigNoz increment will bind Nexus resource identity to planned maintenance after the deployed alert-label mapping is verified, then add managed alert-rule lifecycle on the validated `/api/v2/rules` schema.
