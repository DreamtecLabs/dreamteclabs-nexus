# Monitoring status model

Nexus separates inventory intent from observed telemetry. Inventory state controls whether a target is enabled, in maintenance or disabled; SigNoz telemetry remains the observed source for packet loss and latency. This prevents UI state from being mistaken for device health.
