# Monitoring test coverage

Backend unit tests cover conservative address validation, closed monitoring-state validation, and generation of per-device ICMP pipelines that exclude maintenance targets. Existing Nexus CI runs all `api::nexus` server tests and a full server `cargo check`; the UI package workflow compiles the Yew frontend. Static monitoring validation also verifies API wiring, navigation, SigNoz credential handling and service packaging.
