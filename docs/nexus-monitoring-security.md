# Monitoring security notes

- SigNoz credentials are read only by the Nexus backend from `NEXUS_SIGNOZ_API_KEY`; they are never persisted in the monitoring inventory or returned to the UI.
- Device addresses are validated as IP addresses or conservative DNS hostnames before they are persisted or passed to `ping`.
- The backend invokes executables directly with argument arrays. Device values are never passed through a shell.
- ICMP collector configuration is generated from validated inventory data, validated by `otelcol-contrib` before replacement, and written atomically.
- Read operations use `PRIV_SYS_AUDIT`; inventory mutations and reconciliation use `PRIV_SYS_MODIFY`.
- Maintenance and disabled devices are excluded from active ICMP pipelines rather than relying on a UI-only flag.
