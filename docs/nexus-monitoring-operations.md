# Monitoring operations

Normal device administration happens in the Nexus **Monitoring** screen. Operators should not edit `nexus-icmp-collector.yaml` directly.

A device save/delete/state change persists the Nexus inventory and immediately attempts collector reconciliation. If reconciliation fails, the inventory remains saved and the UI reports the failure. This makes collector installation or transient service errors recoverable through the **Reconcile probes** action without re-entering devices.

A one-shot **Probe now** action is provided for troubleshooting and does not replace continuous OpenTelemetry collection.
