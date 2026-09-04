# Monitoring deployment dependency

The Nexus backend package installs the dedicated `nexus-icmp-collector.service` unit, but OpenTelemetry Collector Contrib itself remains an external runtime dependency. The Monitoring UI reports whether `/usr/bin/otelcol-contrib` is installed and preserves device inventory when reconciliation cannot run.
