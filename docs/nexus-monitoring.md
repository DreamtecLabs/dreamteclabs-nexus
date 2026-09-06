# Nexus Monitoring

Nexus owns the monitoring control plane. SigNoz remains the observability backend and OpenTelemetry remains the telemetry transport/runtime.

## Responsibilities

- **Nexus UI/API**: source of truth for monitored devices, Prometheus services, monitoring state and provider configuration.
- **SigNoz**: metrics exploration, dashboards, alert rules, planned maintenance and notification routing.
- **Prometheus Blackbox Exporter**: ICMP synthetic probing for agentless devices.
- **OpenTelemetry Collector**: collects blackbox and Prometheus service metrics and exports them to SigNoz over OTLP; it also remains the host-metrics path for Linux guests.

Operators should not edit monitoring YAML to register targets. Add, remove or change targets through the Nexus monitoring API/UI. The backend regenerates and validates the OpenTelemetry collector configuration and reconciles the required services.

## Monitoring inventory

The backend persists Nexus-owned state in `monitoring.json` under the PDM configuration directory.

Device records contain a stable Nexus id, human-readable name, IPv4/IPv6/hostname, device kind, site, monitoring profile (`icmp`) and state (`enabled`, `maintenance` or `disabled`).

Service records contain a stable Nexus id, human-readable name, IPv4/IPv6/hostname, TCP port, Prometheus metrics path, site, monitoring profile (`prometheus`) and the same monitoring state. Existing inventories are upgraded in memory with an empty `services` collection, so adding service monitoring is backward compatible with deployed `monitoring.json` files.

Only `enabled` targets are rendered into the active collector configuration. Maintenance and disabled targets are intentionally excluded from collection.

## ICMP probe engine

The supported agentless ICMP path is:

`Nexus inventory -> Prometheus Blackbox Exporter -> OpenTelemetry Prometheus receiver -> OTLP -> SigNoz`

Debian Trixie's `prometheus-blackbox-exporter` package supplies the ICMP prober and `/etc/prometheus/blackbox.yml`. Nexus depends on that package instead of relying on the experimental OpenTelemetry `icmpcheckreceiver`, which is not present in the deployed `otelcol-contrib 0.139.0` distribution.

The backend generates `/etc/proxmox-datacenter-manager/nexus-icmp-collector.yaml` for `nexus-icmp-collector.service`. The filename and unit name are retained for compatibility, but the generated collector now owns both ICMP and managed Prometheus service receivers.

Before replacing the active collector configuration, Nexus validates both the Debian blackbox configuration (when ICMP devices exist) and the generated OpenTelemetry configuration. If validation fails, the target stays persisted but the invalid collector configuration is not promoted.

When at least one monitoring target is enabled, reconcile enables/starts `nexus-icmp-collector.service`. The blackbox exporter is enabled only when at least one ICMP device is active and is stopped when only Prometheus services remain. When no targets are enabled, Nexus disables/stops both services. This keeps service lifecycle owned by Nexus rather than by manual operator edits.

The collector exports OTLP metrics to `192.168.0.47:4317` by default.

## Prometheus service monitoring

Managed Prometheus services are scraped directly by the OpenTelemetry Collector. Each enabled service gets a dedicated Prometheus scrape job using its configured address, port and metrics path. Nexus attaches stable labels to every resulting time series:

- `nexus_service_id`
- `nexus_service_name`
- `nexus_resource_type="service"`
- `nexus_site`
- `nexus_monitoring_profile="prometheus"`

This is the path used for DreamtecLabs Notify. Notify exposes WhatsApp session metrics such as `dreamteclabs_notify_whatsapp_instance_open`, while the Prometheus receiver also emits scrape availability (`up`). Together these support per-instance failure alerting and loss-of-telemetry detection without exposing credentials, phone numbers, messages or QR data.

## SigNoz API

The Nexus backend reads:

- `NEXUS_SIGNOZ_URL` (defaults to `http://192.168.0.47:8080`)
- `NEXUS_SIGNOZ_API_KEY` (required for authenticated SigNoz API calls)

The key stays server-side and is never returned to the browser. Nexus uses the current SigNoz rules API at `/api/v2/rules` for rule discovery and the planned-maintenance API at `/api/v1/downtime_schedules` for downtime lifecycle operations.

SigNoz remains authoritative for alert definitions. Nexus does not copy rule bodies into its own inventory. Planned maintenance created through Nexus is sent directly to SigNoz with a fixed schedule, optional alert rule IDs and an optional SigNoz label-scope expression. This preserves SigNoz evaluation while suppressing matching notifications during the maintenance window.

The backend deliberately validates only Nexus-owned input boundaries and lets SigNoz validate RFC3339 timestamps, IANA timezones and scope-expression semantics. Provider error bodies and HTTP statuses are propagated back to the operator instead of being hidden.

## API routes

- `GET /api2/json/monitoring` — inventory and local collector status
- `GET /api2/json/monitoring/signoz` — safe SigNoz API connectivity summary
- `GET /api2/json/monitoring/signoz-rules` — current SigNoz alert rules
- `GET /api2/json/monitoring/signoz-downtimes` — current SigNoz planned-maintenance windows
- `POST /api2/json/monitoring/signoz-downtime` — create a fixed planned-maintenance window
- `POST /api2/json/monitoring/signoz-downtime-delete` — delete a planned-maintenance window
- `POST /api2/json/monitoring/device` — create/update an ICMP device and reconcile
- `POST /api2/json/monitoring/device-delete` — delete a device and reconcile
- `POST /api2/json/monitoring/device-probe` — one-shot diagnostic ping
- `POST /api2/json/monitoring/service` — create/update a Prometheus service and reconcile
- `POST /api2/json/monitoring/service-delete` — delete a Prometheus service and reconcile
- `POST /api2/json/monitoring/reconcile` — regenerate/restart the managed collector

Read operations require system audit privileges. Mutating operations require system modify privileges.

## Automatic maintenance binding

Every ICMP metric for a device carries `nexus_device_id`, and every managed Prometheus service metric carries `nexus_service_id`. Switching a managed target to `maintenance` creates a SigNoz planned-maintenance window scoped to exactly that target label, excludes the target from collection and persists the returned `signoz_downtime_id` in `monitoring.json`.

Switching the target back to `enabled` or `disabled` deletes that SigNoz downtime automatically. Deleting the target also removes any associated downtime. If a SigNoz call fails, the local state change and collector reconciliation still proceed and the error is surfaced in `signoz_maintenance.downtime_error`; tracked downtime ids are retained until deletion succeeds, avoiding orphaned suppression windows.

The explicit `signoz-downtime`/`signoz-downtime-delete` API routes remain available for maintenance windows that are not tied to a single Nexus target.

## Alerting model

For DreamtecLabs Notify, the intended SigNoz rules are:

- `dreamteclabs_notify_whatsapp_instance_open == 0` grouped by `instance`, sustained for two minutes.
- `dreamteclabs_notify_evolution_up == 0` sustained for one to two minutes.
- scrape/telemetry loss using the Prometheus `up` metric for `nexus_service_id="dreamteclabs-notify"`, so a dead Notify process or unreachable `/metrics` cannot silently look healthy.

The exact rule creation payload remains SigNoz-version-specific and should be validated against the deployed `/api/v2/rules` contract before Nexus starts creating rules automatically. Until that contract is confirmed, SigNoz remains authoritative for rule bodies while Nexus owns target lifecycle and maintenance scopes.

## Next increments

The same inventory model can expand to HTTP/HTTPS, TCP and SNMP profiles. The next monitoring increment is managed SigNoz alert-rule creation after validating the deployed rule-create payload and no-data semantics end to end.
