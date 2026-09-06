# Nexus Monitoring

Nexus owns the monitoring control plane. SigNoz remains the observability backend and OpenTelemetry remains the telemetry transport/runtime.

## Responsibilities

- **Nexus UI/API**: source of truth for monitored devices, monitoring state and provider configuration.
- **SigNoz**: metrics exploration, dashboards, alert rules, planned maintenance and notification routing.
- **Prometheus Blackbox Exporter**: ICMP synthetic probing for agentless devices.
- **OpenTelemetry Collector**: collects blackbox metrics and exports them to SigNoz over OTLP; it also remains the host-metrics path for Linux guests.

Operators should not edit monitoring YAML to register devices. Add, remove or change devices through **Monitoring** in the Nexus UI. The backend regenerates and validates the OpenTelemetry collector configuration and reconciles the required services.

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

The supported agentless ICMP path is:

`Nexus inventory -> Prometheus Blackbox Exporter -> OpenTelemetry Prometheus receiver -> OTLP -> SigNoz`

Debian Trixie's `prometheus-blackbox-exporter` package supplies the ICMP prober and `/etc/prometheus/blackbox.yml`. Nexus depends on that package instead of relying on the experimental OpenTelemetry `icmpcheckreceiver`, which is not present in the deployed `otelcol-contrib 0.139.0` distribution.

The backend generates `/etc/proxmox-datacenter-manager/nexus-icmp-collector.yaml` for `nexus-icmp-collector.service`. The generated collector uses the supported `prometheus` receiver to scrape the local blackbox exporter on `127.0.0.1:9115`, passing each enabled Nexus device as an ICMP probe target and attaching Nexus identity labels.

Before replacing the active collector configuration, Nexus validates both the Debian blackbox configuration and the generated OpenTelemetry configuration. If validation fails, the device stays persisted but the invalid collector configuration is not promoted.

When at least one device is enabled, reconcile enables/starts `prometheus-blackbox-exporter.service` and `nexus-icmp-collector.service`. When no devices are enabled, Nexus disables/stops both services. This keeps service lifecycle owned by Nexus rather than by manual operator edits.

The collector exports OTLP metrics to `192.168.0.47:4317` by default.

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

## Automatic per-device maintenance binding

Every metric the ICMP probe pipeline emits for a device carries a `nexus_device_id` label matching that device's Nexus id (see `collector::collector_config`). This has been validated against the deployed SigNoz instance as the resource's real alert-label identity.

Switching a device to the local `maintenance` state (via `POST /monitoring/device`) now automatically:

- creates a SigNoz planned-maintenance window scoped to exactly `nexus_device_id="<id>"`, so only that device's alerts are suppressed — no other resource is affected
- stops the ICMP probe for that device (unchanged: only `enabled` devices are rendered into the active probe configuration)
- records the created downtime's id on the device record (`signoz_downtime_id` in `monitoring.json`)

Switching the device back out of `maintenance` (`enabled` or `disabled`) deletes that SigNoz downtime automatically, so alerting resumes without operator intervention. Deleting the device entirely also removes any associated downtime.

If the SigNoz API call fails (unreachable, misconfigured key, etc.), the local state change and probe reconciliation still succeed — the operator sees the failure in the response's `signoz_maintenance.downtime_error` field and can retry by toggling the device's state again. The device keeps tracking a downtime id it failed to delete so a later attempt can still find it, instead of leaking an orphaned SigNoz schedule.

The explicit `signoz-downtime`/`signoz-downtime-delete` API routes remain available for maintenance windows that are not tied to a single Nexus device (e.g. a host-wide or manually scoped window).

## Next increments

The data model is designed to expand to HTTP/HTTPS, TCP and SNMP profiles without making SigNoz the inventory source of truth. The next SigNoz increment is a managed availability alert rule (`probe_success == 0`) that also respects this same per-device maintenance binding.
