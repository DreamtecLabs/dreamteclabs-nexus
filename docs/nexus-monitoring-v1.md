# Monitoring v1 acceptance criteria

- Monitoring is reachable from the Nexus navigation.
- SigNoz API credentials remain server-side.
- Operators can add and remove ICMP devices from the UI.
- Operators can place a device in maintenance and re-enable it from the UI.
- Enabled devices are represented by isolated OpenTelemetry ICMP pipelines.
- Collector configuration is validated before activation.
- A manual probe can be run from the UI.
- Backend and UI compile in CI before merge.
