# Monitoring source of truth

Nexus inventory is authoritative for what should be monitored. OpenTelemetry is the collection runtime and SigNoz is authoritative for observed telemetry and alert evaluation. Operators do not maintain a second manual target inventory in collector YAML.
