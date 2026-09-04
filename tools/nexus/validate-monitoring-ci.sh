#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

grep -q 'pub mod monitoring;' server/src/api/nexus/mod.rs
grep -q 'nexus::monitoring::ROUTER' server/src/api/mod.rs
grep -q 'API_METHOD_SIGNOZ_STATUS' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_LIST_SIGNOZ_RULES' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_LIST_SIGNOZ_DOWNTIMES' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_CREATE_SIGNOZ_DOWNTIME' server/src/api/nexus/monitoring/mod.rs
grep -q 'API_METHOD_DELETE_SIGNOZ_DOWNTIME' server/src/api/nexus/monitoring/mod.rs
grep -q 'NEXUS_SIGNOZ_API_KEY' server/src/api/nexus/monitoring/signoz.rs
grep -q 'SIGNOZ-API-KEY' server/src/api/nexus/monitoring/signoz.rs
grep -q 'const RULES_PATH: &str = "/api/v2/rules"' server/src/api/nexus/monitoring/signoz.rs
grep -q 'const DOWNTIME_PATH: &str = "/api/v1/downtime_schedules"' server/src/api/nexus/monitoring/signoz.rs
grep -q 'Method::POST, DOWNTIME_PATH' server/src/api/nexus/monitoring/signoz.rs
grep -q 'Method::DELETE' server/src/api/nexus/monitoring/signoz.rs
grep -q 'prometheus/' server/src/api/nexus/monitoring/collector.rs
grep -q 'module: \[icmp\]' server/src/api/nexus/monitoring/collector.rs
grep -q '127.0.0.1:9116' server/src/api/nexus/monitoring/collector.rs
grep -q 'nexus.device.id' server/src/api/nexus/monitoring/collector.rs
grep -q 'maintenance' server/src/api/nexus/monitoring/store.rs
grep -q 'NexusMonitoring' ui/src/nexus/mod.rs
grep -q '"Monitoring"' ui/src/main_menu.rs
grep -q '"/monitoring/device"' ui/src/nexus/monitoring.rs

test -f services/nexus-icmp-collector.service
grep -q 'otelcol-contrib' services/nexus-icmp-collector.service
test -f services/nexus-blackbox-exporter.service
grep -q 'prometheus-blackbox-exporter' services/nexus-blackbox-exporter.service
grep -q '127.0.0.1:9116' services/nexus-blackbox-exporter.service
grep -q 'CAP_NET_RAW' services/nexus-blackbox-exporter.service
grep -q 'nexus-blackbox-exporter.service' services/Makefile
grep -q 'nexus-icmp-collector.service' services/Makefile
grep -q 'nexus-blackbox-exporter.service' debian/proxmox-datacenter-manager.install
grep -q 'nexus-icmp-collector.service' debian/proxmox-datacenter-manager.install
grep -q 'nexus-blackbox-exporter.service' debian/proxmox-datacenter-manager.postinst

grep -q 'nexus-icmp-collector.service' tools/nexus/build-backend-container.sh
grep -q 'nexus-blackbox-exporter.service' tools/nexus/build-backend-container.sh

cargo fmt --all -- --check
