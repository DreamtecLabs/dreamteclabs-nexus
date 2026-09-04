# Monitoring device model

The initial device profile is `icmp`. Every device has a stable Nexus id, display name, address, type, site and state.

States:

- `enabled`: included in the generated OpenTelemetry ICMP pipelines.
- `maintenance`: intentionally excluded from probing while maintenance is in progress.
- `disabled`: retained in inventory but excluded from probing.

The model deliberately keeps the monitoring profile separate from device type so future HTTP, TCP and SNMP checks can be added without changing inventory identity.
