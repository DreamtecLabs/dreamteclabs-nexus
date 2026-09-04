# Monitoring separation of concerns

PDM/Nexus controls inventory and intent. OpenTelemetry performs collection. SigNoz stores, queries, visualizes and alerts. This separation keeps monitoring extensible while avoiding direct provider logic in the UI.
