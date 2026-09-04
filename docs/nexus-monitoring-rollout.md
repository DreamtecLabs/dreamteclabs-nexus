# Nexus Monitoring rollout checklist

1. Deploy the backend and UI artifacts produced from the same merged master commit.
2. Install `otelcol-contrib` on the Nexus/PDM host if it is not already present.
3. Configure a SigNoz service-account API key as `NEXUS_SIGNOZ_API_KEY` for the PDM API services. `NEXUS_SIGNOZ_URL` is optional and defaults to the current internal SigNoz URL.
4. Open **Monitoring** in Nexus and confirm the SigNoz API connection card is healthy.
5. Add one low-risk ICMP target through the UI and confirm `nexus-icmp-collector.service` becomes active.
6. Confirm `ping.loss.ratio` and `ping.rtt.avg` arrive in SigNoz with `nexus.device.id`, `nexus.device.name`, `nexus.resource.type`, `nexus.site` and `nexus.monitoring.profile` attributes.
7. Put the test device into Maintenance and confirm it is removed from the generated collector configuration after reconciliation.
8. Re-enable it and confirm telemetry resumes.
9. Add the remaining household/network devices only through Nexus; do not maintain a parallel manual YAML target list.
