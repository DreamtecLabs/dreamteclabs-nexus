# Nexus Monitoring API contract

The Monitoring frontend talks only to Nexus authenticated API routes. Browser code never receives the SigNoz API key and never writes OpenTelemetry configuration directly.

The initial contract is intentionally small:

- `GET /monitoring` returns the persisted inventory plus local ICMP collector status.
- `GET /monitoring/signoz` checks the server-side SigNoz API integration and returns only URL, configured/connected state, rule count and a safe error string.
- `POST /monitoring/device` upserts an ICMP device. Parameters: `name`, `address`, `kind`, optional `site`, optional `state`.
- `POST /monitoring/device-delete` removes a device by Nexus `id`.
- `POST /monitoring/device-probe` performs a one-shot diagnostic ping by Nexus `id`.
- `POST /monitoring/reconcile` regenerates and applies the collector configuration from the persisted inventory.

Device mutations persist first and reconcile second. If the local collector cannot be reconciled, the inventory is retained and the API response includes the reconciliation error so the UI can surface an actionable state without losing operator input.
