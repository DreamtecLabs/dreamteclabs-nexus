# Nexus Monitoring

Nexus owns the monitoring control plane. SigNoz remains the observability backend and OpenTelemetry remains the telemetry transport/runtime.

## Responsibilities

- **Nexus UI/API**: source of truth for monitored devices, monitoring state and provider configuration.
- **SigNoz**: metrics exploration, dashboards, alert rules, planned maintenance and notification routing.
- **Prometheus Blackbox Exporter**: stable ICMP probe execution for agentless devices.
- **OpenTelemetry Collector**: scrapes Blackbox Exporter metrics through its supported Prometheus receiver and exports them to SigNoz over OTLP.

Operators should not edit probe or collector configuration to register devices. Add, remove or change devices through **Monitoring** in the Nexus UI. The backend regenerates and validates the collector configuration and reconciles the dedicated Nexus services.

## Device inventory

The backend persists Nexus-owned state in `monitoring.json` under the PDM configuration directory. Device records contain:

- stable Nexus id
- human-readable name
- IPv4, IPv6 or hostname
- device kind
- site
- monitoring profile (`icmp` in the initial implementation)
- state: `enabled`, `maintenance` or `disabled`

Only `enabled` devices are rendered into the active probe configuration. Maintenance and disabled devices are intentionally excluded from probes.

## ICMP probe engine

Nexus deliberately does not depend on the experimental OpenTelemetry ICMP receiver. The supported runtime path is:

`Nexus inventory -> Prometheus Blackbox Exporter (ICMP) -> OpenTelemetry Prometheus receiver -> OTLP -> SigNoz`

The packaged `nexus-blackbox-exporter.service` binds Blackbox Exporter to `127.0.0.1:9116` and uses the Debian package's standard `/etc/prometheus/blackbox.yml` ICMP module. It is not exposed to the LAN.

The backend generates `/etc/proxmox-datacenter-manager/nexus-icmp-collector.yaml` for `nexus-icmp-collector.service`. Each enabled device receives an isolated `prometheus/<device-id>` receiver and metrics pipeline. The receiver calls the local Blackbox `/probe` endpoint with `module=icmp`, while Nexus resource processors attach `nexus.device.id`, `nexus.device.name`, `nexus.resource.type`, `nexus.site` and `nexus.monitoring.profile` before export.

The collector exports OTLP metrics to `192.168.0.47:4317` by default. Before replacing active configuration, Nexus validates the generated YAML with `/usr/bin/otelcol-contrib validate`. A failed validation never replaces the previous active configuration and the complete validation failure is returned to the UI.

Both `/usr/bin/otelcol-contrib` and `/usr/bin/prometheus-blackbox-exporter` are runtime prerequisites. Nexus device CRUD does not silently install operating-system packages. If either prerequisite is absent, the device remains safely persisted and the UI surfaces the reconciliation failure.

When there are no enabled targets Nexus disables both `nexus-blackbox-exporter.service` and `nexus-icmp-collector.service` and removes the generated collector YAML. With enabled targets, Nexus starts the Blackbox service first and the collector second.

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
