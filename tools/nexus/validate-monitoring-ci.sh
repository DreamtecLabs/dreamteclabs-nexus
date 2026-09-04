#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

grep -q 'pub mod monitoring;' server/src/api/nexus/mod.rs
grep -q 'nexus::monitoring::ROUTER' server/src/api/mod.rs
grep -q 'API_METHOD_SIGNOZ_STATUS' server/src/api/nexus/monitoring.rs
grep -q 'NEXUS_SIGNOZ_API_KEY' server/src/api/nexus/monitoring.rs
grep -q 'SIGNOZ-API-KEY' server/src/api/nexus/monitoring.rs
grep -q 'icmpcheck/' server/src/api/nexus/monitoring.rs
grep -q 'nexus.device.id' server/src/api/nexus/monitoring.rs
grep -q 'maintenance' server/src/api/nexus/monitoring.rs
grep -q 'NexusMonitoring' ui/src/nexus/mod.rs
grep -q '"Monitoring"' ui/src/main_menu.rs
grep -q '"/monitoring/device"' ui/src/nexus/monitoring.rs

test -f services/nexus-icmp-collector.service
grep -q 'otelcol-contrib' services/nexus-icmp-collector.service
grep -q 'CAP_NET_RAW' services/nexus-icmp-collector.service
grep -q 'nexus-icmp-collector.service' services/Makefile
grep -q 'nexus-icmp-collector.service' debian/proxmox-datacenter-manager.install

grep -q 'nexus-icmp-collector.service' tools/nexus/build-backend-container.sh

cargo fmt --all -- --check
